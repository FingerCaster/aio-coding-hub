//! Channel IPC operations share the native publisher's file/intent transaction.
use crate::app_state::{ensure_db_ready, DbInitState};
use crate::domain::native_channels::*;
use crate::domain::native_cli::NativeTarget;
use crate::domain::native_gateway::{
    GatewayLifecycleInput, GatewayModelsSnapshot, NativeModelSpec,
};
use crate::infra::native_cli::{self, node_digest};
use crate::shared::error::{AppError, AppResult};
use crate::shared::gateway_protocol::GatewayProtocol;
use crate::{blocking, db};
use rusqlite::TransactionBehavior;

pub(crate) fn catalog_for_target(
    db: &db::Db,
    target: &NativeTarget,
    origin: Option<&str>,
    policy: &str,
    check: impl FnOnce() -> AppResult<()>,
) -> AppResult<ChannelCatalog> {
    native_cli::with_target_lock(target, |session| {
        check()?;
        let snapshot = session.snapshot()?;
        let conn = db.open_connection()?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| crate::shared::error::db_err!("channel catalog snapshot: {e}"))?;
        let (catalog_revision, sources) =
            channel_catalog(&tx, target.client.as_str(), origin, policy)?;
        let bindings = binding_summaries(
            &list_bindings(&tx, &target.target_id)?,
            snapshot.providers(),
            &catalog_revision,
        );
        Ok(ChannelCatalog {
            target_id: target.target_id.clone(),
            revision: snapshot.revision,
            catalog_revision,
            listener_ready: origin.is_some(),
            sources,
            bindings,
        })
    })
}

pub(crate) fn preview_for_target(
    db: &db::Db,
    target: &NativeTarget,
    input: &ChannelLifecycleInput,
    origin: Option<&str>,
    policy: &str,
    check: impl FnOnce() -> AppResult<()>,
) -> AppResult<ChannelPreview> {
    native_cli::with_target_lock(target, |session| {
        check()?;
        let snapshot = session.snapshot()?;
        if input.target_id != target.target_id || input.expected_revision != snapshot.revision {
            return Err(AppError::new(
                "NATIVE_REVISION_CONFLICT",
                "Reload the selected native target before previewing",
            ));
        }
        if !target.writable {
            return Err(AppError::new(
                "NATIVE_TARGET_READ_ONLY",
                "This native target is read-only",
            ));
        }
        let conn = db.open_connection()?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| crate::shared::error::db_err!("channel preview snapshot: {e}"))?;
        let plan = channel_plan(&tx, target.client.as_str(), input, origin, policy)?;
        for old in &plan.previous {
            if old.state != "applied" {
                let actual = snapshot.providers().get(&old.native_key).map(node_digest);
                let desired = old.payload.desired.as_ref().map(|e| node_digest(&e.node));
                let previous = old
                    .payload
                    .previous
                    .as_ref()
                    .map(|v| node_digest(&v.entry.node));
                // Preview the same exact-node recovery that apply performs,
                // without finalizing or rolling back any durable intent here.
                if actual != desired && actual != previous {
                    return Err(AppError::new(
                        "NATIVE_GATEWAY_RECOVERY_CONFLICT",
                        "Pending channel entry was externally modified",
                    ));
                }
                continue;
            }
            if snapshot
                .providers()
                .get(&old.native_key)
                .map(node_digest)
                .as_deref()
                != Some(&old.node_digest)
            {
                return Err(AppError::new(
                    "NATIVE_GATEWAY_NODE_CONFLICT",
                    "Owned channel entry was externally modified",
                ));
            }
        }
        for (_, entry) in &plan.desired {
            if !plan
                .previous
                .iter()
                .any(|m| m.native_key == entry.native_key)
                && snapshot.providers().contains_key(&entry.native_key)
            {
                return Err(AppError::new(
                    "NATIVE_GATEWAY_NODE_CONFLICT",
                    "Channel entry name is already in use",
                ));
            }
        }
        let removed_native_keys = plan
            .previous
            .iter()
            .filter(|m| {
                !plan
                    .desired
                    .iter()
                    .any(|(_, e)| e.native_key == m.native_key)
            })
            .map(|m| m.native_key.clone())
            .collect();
        Ok(ChannelPreview {
            target_id: target.target_id.clone(),
            revision: snapshot.revision,
            catalog_revision: plan.revision,
            entries: plan.desired.into_iter().map(|(_, e)| e).collect(),
            removed_native_keys,
        })
    })
}

pub(crate) fn apply_for_target(
    db: &db::Db,
    target: &NativeTarget,
    input: &ChannelLifecycleInput,
    origin: Option<&str>,
    policy: &str,
    check: impl Fn() -> AppResult<()>,
) -> AppResult<ChannelMutationResult> {
    let legacy = GatewayLifecycleInput {
        target_id: input.target_id.clone(),
        expected_revision: input.expected_revision.clone(),
        catalog_revision: input.catalog_revision.clone(),
    };
    let result = super::native_gateway_service::mutate_owned_with_guard(
        db,
        target,
        &legacy,
        origin,
        policy,
        input.selections.is_empty(),
        Some(input),
        check,
    )?;
    Ok(ChannelMutationResult {
        target_id: result.gateway.target_id,
        revision: result.gateway.revision,
        changed: result.gateway.changed,
        backup_path: result.gateway.backup_path,
        bindings: result.bindings,
    })
}

pub(crate) async fn native_channel_catalog_preview(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    target_id: String,
) -> Result<ChannelCatalog, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("native_channel_catalog_preview", move || {
        let target = super::native_cli_service::resolve_target(&app, &target_id)?;
        let (origin, policy) = super::native_gateway_service::context(&app)?;
        catalog_for_target(&db, &target, origin.as_deref(), &policy, || {
            super::native_cli_service::verify_selected_target(&app, &target)
        })
    })
    .await
    .map_err(Into::into)
}
pub(crate) async fn native_channel_preview(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: ChannelLifecycleInput,
) -> Result<ChannelPreview, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("native_channel_preview", move || {
        let target = super::native_cli_service::resolve_target(&app, &input.target_id)?;
        let (origin, policy) = super::native_gateway_service::context(&app)?;
        preview_for_target(&db, &target, &input, origin.as_deref(), &policy, || {
            super::native_cli_service::verify_selected_target(&app, &target)
        })
    })
    .await
    .map_err(Into::into)
}
pub(crate) async fn native_channel_apply(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: ChannelLifecycleInput,
) -> Result<ChannelMutationResult, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("native_channel_apply", move || {
        let target = super::native_cli_service::resolve_target(&app, &input.target_id)?;
        let (origin, policy) = super::native_gateway_service::context(&app)?;
        apply_for_target(&db, &target, &input, origin.as_deref(), &policy, || {
            super::native_cli_service::verify_selected_target(&app, &target)?;
            if !input.selections.is_empty()
                && super::native_gateway_service::context(&app)? != (origin.clone(), policy.clone())
            {
                return Err(AppError::new(
                    "NATIVE_GATEWAY_CATALOG_CONFLICT",
                    "Listener or model policy changed; preview again",
                ));
            }
            Ok(())
        })
    })
    .await
    .map_err(Into::into)
}
pub(crate) async fn native_channel_models_get(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    target_id: String,
    provider_id: i64,
    provider_uuid: String,
    protocol: GatewayProtocol,
) -> Result<GatewayModelsSnapshot, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("native_channel_models_get", move || {
        let target = super::native_cli_service::resolve_target(&app, &target_id)?;
        native_cli::with_target_lock(&target, |_| {
            super::native_cli_service::verify_selected_target(&app, &target)?;
            let conn = db.open_connection()?;
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| crate::shared::error::db_err!("channel models snapshot: {e}"))?;
            channel_models_get(
                &tx,
                provider_id,
                &provider_uuid,
                target.client.as_str(),
                protocol,
            )
        })
    })
    .await
    .map_err(Into::into)
}
#[allow(clippy::too_many_arguments)]
pub(crate) async fn native_channel_models_set(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    target_id: String,
    provider_id: i64,
    provider_uuid: String,
    protocol: GatewayProtocol,
    expected_revision: String,
    models: Vec<NativeModelSpec>,
) -> Result<GatewayModelsSnapshot, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("native_channel_models_set", move || {
        let target = super::native_cli_service::resolve_target(&app, &target_id)?;
        native_cli::with_target_lock(&target, |_| {
            super::native_cli_service::verify_selected_target(&app, &target)?;
            let mut conn = db.open_connection()?;
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|e| crate::shared::error::db_err!("channel models transaction: {e}"))?;
            let result = channel_models_set(
                &tx,
                provider_id,
                &provider_uuid,
                target.client.as_str(),
                protocol,
                &expected_revision,
                &models,
            )?;
            tx.commit()
                .map_err(|e| crate::shared::error::db_err!("channel models commit: {e}"))?;
            Ok(result)
        })
    })
    .await
    .map_err(Into::into)
}

#[cfg(test)]
#[path = "native_channel_service_tests.rs"]
mod tests;
