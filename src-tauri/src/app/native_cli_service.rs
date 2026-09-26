//! Native configuration orchestration: settings-owned targets, DB archives,
//! and node-scoped conditional compensation. Never consult native auth stores.
use crate::app_state::{ensure_db_ready, DbInitState};
use crate::domain::native_cli::*;
use crate::infra::native_cli::{
    self, node_digest, profiles, targets, NativeDocumentSnapshot, NativeNodePatch,
    NativePatchReceipt, NativeTargetSession,
};
use crate::shared::error::{AppError, AppResult};
use crate::{blocking, db, settings};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

fn env_value(name: &str) -> AppResult<Option<String>> {
    match std::env::var(name) {
        Ok(v) => Ok(Some(v)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(_) => Err(AppError::new(
            "NATIVE_TARGET_INVALID",
            "Native directory environment is not valid Unicode",
        )),
    }
}

fn candidates<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    client: NativeClient,
) -> AppResult<Vec<NativeTarget>> {
    let config = settings::read(app)?;
    let home = crate::app_paths::home_dir(app)?;
    let agent_env = env_value("PI_CODING_AGENT_DIR")?;
    let config_env = env_value("PI_CONFIG_DIR")?;
    let omp_profile_env = env_value("OMP_PROFILE")?;
    let pi_profile_env = env_value("PI_PROFILE")?;
    let selected = config
        .pi_omp_native_targets
        .iter()
        .find(|item| item.client == client)
        .cloned()
        .unwrap_or_else(|| NativeTargetSelection::default_for(client));
    let selected_target = targets::resolve_with_profiles(
        &home,
        &selected,
        agent_env.as_deref(),
        config_env.as_deref(),
        omp_profile_env.as_deref(),
        pi_profile_env.as_deref(),
    )?;
    let mut selections = vec![NativeTargetSelection::default_for(client), selected];
    if client == NativeClient::Omp {
        // A broken unrelated profile root must not prevent recovery through an
        // already validated custom target. The selected target resolved above.
        if let Ok(profiles) = targets::discover_profiles(&home, config_env.as_deref()) {
            selections.extend(profiles);
        }
    }
    let other_client = match client {
        NativeClient::Pi => NativeClient::Omp,
        NativeClient::Omp => NativeClient::Pi,
    };
    let other_selection = config
        .pi_omp_native_targets
        .iter()
        .find(|item| item.client == other_client)
        .cloned()
        .unwrap_or_else(|| NativeTargetSelection::default_for(other_client));
    let other = targets::resolve_with_profiles(
        &home,
        &other_selection,
        agent_env.as_deref(),
        config_env.as_deref(),
        omp_profile_env.as_deref(),
        pi_profile_env.as_deref(),
    )
    .ok();
    let mut result = Vec::new();
    for selection in selections {
        let target = targets::resolve_with_profiles(
            &home,
            &selection,
            agent_env.as_deref(),
            config_env.as_deref(),
            omp_profile_env.as_deref(),
            pi_profile_env.as_deref(),
        );
        // The selected target already resolved above. An invalid, unselected
        // default must not prevent recovery by choosing a valid custom target.
        let Ok(mut target) = target else {
            continue;
        };
        if result
            .iter()
            .any(|item: &NativeTarget| item.target_id == target.target_id)
        {
            continue;
        }
        target.selected = target.target_id == selected_target.target_id;
        mark_collision(&mut target, other.as_ref());
        result.push(target);
    }
    Ok(result)
}

pub(crate) fn resolve_target<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    target_id: &str,
) -> AppResult<NativeTarget> {
    if target_id.len() != 64 || !target_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(AppError::new(
            "NATIVE_TARGET_NOT_FOUND",
            "Reload and choose a native configuration target",
        ));
    }
    for client in [NativeClient::Pi, NativeClient::Omp] {
        let Ok(candidates) = candidates(app, client) else {
            continue;
        };
        if let Some(target) = candidates
            .into_iter()
            .find(|t| t.selected && t.target_id == target_id)
        {
            return Ok(target);
        }
    }
    Err(AppError::new(
        "NATIVE_TARGET_NOT_FOUND",
        "Native target changed or is no longer selected; reload targets",
    ))
}

/// Recheck after obtaining the target lock: a queued preview may have been
/// superseded by a completed target selection change.
pub(crate) fn verify_selected_target<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    target: &NativeTarget,
) -> AppResult<()> {
    let selected = resolve_target(app, &target.target_id)?;
    if target.writable && !selected.writable {
        return Err(AppError::new(
            "NATIVE_TARGET_CHANGED",
            "Native target became read-only; reload targets",
        ));
    }
    Ok(())
}

fn validate_target<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    selection: &NativeTargetSelection,
) -> AppResult<NativeTarget> {
    let home = crate::app_paths::home_dir(app)?;
    let agent_env = env_value("PI_CODING_AGENT_DIR")?;
    let config_env = env_value("PI_CONFIG_DIR")?;
    let omp_profile_env = env_value("OMP_PROFILE")?;
    let pi_profile_env = env_value("PI_PROFILE")?;
    let mut target = targets::resolve_with_profiles(
        &home,
        selection,
        agent_env.as_deref(),
        config_env.as_deref(),
        omp_profile_env.as_deref(),
        pi_profile_env.as_deref(),
    )?;
    let config = settings::read(app)?;
    let other_client = if selection.client == NativeClient::Pi {
        NativeClient::Omp
    } else {
        NativeClient::Pi
    };
    let other_selection = config
        .pi_omp_native_targets
        .iter()
        .find(|s| s.client == other_client)
        .cloned()
        .unwrap_or_else(|| NativeTargetSelection::default_for(other_client));
    let other = targets::resolve_with_profiles(
        &home,
        &other_selection,
        agent_env.as_deref(),
        config_env.as_deref(),
        omp_profile_env.as_deref(),
        pi_profile_env.as_deref(),
    )
    .ok();
    mark_collision(&mut target, other.as_ref());
    if let Err(error) =
        native_cli::with_target_lock(&target, |session| session.snapshot().map(|_| ()))
    {
        target.writable = false;
        target.issue = Some(error.to_string());
    }
    Ok(target)
}

pub(crate) async fn native_cli_targets_list(
    app: tauri::AppHandle,
    client: NativeClient,
) -> Result<Vec<NativeTarget>, String> {
    blocking::run("native_cli_targets_list", move || candidates(&app, client))
        .await
        .map_err(Into::into)
}
pub(crate) async fn native_cli_target_validate(
    app: tauri::AppHandle,
    selection: NativeTargetSelection,
) -> Result<NativeTarget, String> {
    blocking::run("native_cli_target_validate", move || {
        validate_target(&app, &selection)
    })
    .await
    .map_err(Into::into)
}
fn mark_collision(target: &mut NativeTarget, other: Option<&NativeTarget>) {
    if other.is_some_and(|other| {
        targets::path_identity(Path::new(&other.agent_dir))
            == targets::path_identity(Path::new(&target.agent_dir))
    }) {
        target.writable = false;
        target.issue = Some("NATIVE_TARGET_COLLISION: Pi and OMP resolve to the same agent directory; select separate targets".into());
    }
}

pub(crate) async fn native_cli_target_select(
    app: tauri::AppHandle,
    selection: NativeTargetSelection,
) -> Result<NativeTarget, String> {
    blocking::run(
        "native_cli_target_select",
        move || -> AppResult<NativeTarget> {
            let mut target = validate_target(&app, &selection)?;
            let config = settings::read(&app)?;
            let previous = config
                .pi_omp_native_targets
                .iter()
                .find(|s| s.client == selection.client)
                .cloned()
                .unwrap_or_else(|| NativeTargetSelection::default_for(selection.client));
            let previous_target = targets::resolve_with_profiles(
                &crate::app_paths::home_dir(&app)?,
                &previous,
                env_value("PI_CODING_AGENT_DIR")?.as_deref(),
                env_value("PI_CONFIG_DIR")?.as_deref(),
                env_value("OMP_PROFILE")?.as_deref(),
                env_value("PI_PROFILE")?.as_deref(),
            )
            .ok();
            let persist = || {
                settings::update(&app, |current| {
                    let actual = current
                        .pi_omp_native_targets
                        .iter()
                        .find(|s| s.client == selection.client)
                        .cloned()
                        .unwrap_or_else(|| NativeTargetSelection::default_for(selection.client));
                    if actual != previous {
                        return Err(AppError::new(
                            "NATIVE_TARGET_CHANGED",
                            "Selected target changed concurrently; reload targets",
                        ));
                    }
                    current
                        .pi_omp_native_targets
                        .retain(|item| item.client != selection.client);
                    current.pi_omp_native_targets.push(selection);
                    Ok(())
                })?;
                target.selected = true;
                Ok(target)
            };
            // In-flight old-target operations finish before selection commits.
            // Queued operations recheck selection after acquiring this same lock.
            match previous_target {
                Some(previous_target) => {
                    native_cli::with_target_lock(&previous_target, |_| persist())
                }
                None => persist(), // Invalid old path must still allow recovery.
            }
        },
    )
    .await
    .map_err(Into::into)
}

fn managed_keys(db: &db::Db, target: &NativeTarget) -> AppResult<Vec<String>> {
    crate::domain::native_gateway::managed_native_keys(db, &target.target_id)
}

fn assert_unmanaged(managed: &[String], key: &str) -> AppResult<()> {
    if managed.iter().any(|k| k == key) {
        return Err(AppError::new(
            "NATIVE_MANAGED_NODE",
            "Use the AIO gateway entry lifecycle to modify this managed node",
        ));
    }
    Ok(())
}

fn summary(
    profile: Option<&profiles::Profile>,
    key: &str,
    node: &Value,
    state: NativeProviderState,
    managed: bool,
) -> NativeProviderSummary {
    NativeProviderSummary {
        profile_uuid: profile.map(|p| p.profile_uuid.clone()),
        native_key: key.into(),
        display_name: profile
            .map(|p| p.display_name.clone())
            .unwrap_or_else(|| key.into()),
        state,
        managed,
        node_digest: (state == NativeProviderState::Present).then(|| node_digest(node)),
        profile_revision: profile.map(|p| p.revision.clone()),
        api: node.get("api").and_then(Value::as_str).map(str::to_string),
        model_count: node
            .get("models")
            .and_then(Value::as_array)
            .map_or(0, |m| m.len() as u32),
        api_key_configured: node
            .get("apiKey")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty()),
    }
}

fn list_for_target_checked(
    db: &db::Db,
    target: &NativeTarget,
    verify_selection: impl FnOnce() -> AppResult<()>,
) -> AppResult<NativeProvidersList> {
    native_cli::with_target_lock(target, |session| {
        verify_selection()?;
        let managed = managed_keys(db, target)?;
        let snapshot = session.snapshot();
        let mut conn = db.open_connection()?;
        if let Ok(snapshot) = &snapshot {
            let tx = conn
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(|_| {
                    AppError::new(
                        "NATIVE_ARCHIVE_FAILED",
                        "Cannot synchronize native profiles",
                    )
                })?;
            profiles::sync(&tx, target, snapshot.providers(), &managed)?;
            tx.commit().map_err(|_| {
                AppError::new(
                    "NATIVE_ARCHIVE_FAILED",
                    "Cannot commit native profile synchronization",
                )
            })?;
        }
        let saved = profiles::list(&conn, target)?;
        let mut providers = BTreeMap::new();
        for profile in &saved {
            if managed.contains(&profile.native_key) {
                continue;
            }
            let (node, state) = match &snapshot {
                Ok(snapshot) => match snapshot.providers().get(&profile.native_key) {
                    Some(node) => (node, NativeProviderState::Present),
                    None => (&profile.node, NativeProviderState::Archived),
                },
                Err(_) => (&profile.node, NativeProviderState::Unknown),
            };
            providers.insert(
                profile.native_key.clone(),
                summary(Some(profile), &profile.native_key, node, state, false),
            );
        }
        let (revision, parse_status, issue) = match snapshot {
            Ok(snapshot) => {
                for (key, node) in snapshot.providers() {
                    if managed.contains(key) {
                        providers.insert(
                            key.clone(),
                            summary(None, key, node, NativeProviderState::Present, true),
                        );
                    }
                }
                let status = if !target.writable {
                    NativeParseStatus::ReadOnly
                } else if snapshot.exists {
                    NativeParseStatus::Ready
                } else {
                    NativeParseStatus::Missing
                };
                (Some(snapshot.revision), status, target.issue.clone())
            }
            Err(error) => (
                None,
                if error.code() == "NATIVE_READ_FAILED" {
                    NativeParseStatus::Unreadable
                } else if error.code() == "NATIVE_YAML_READ_ONLY" {
                    NativeParseStatus::ReadOnly
                } else {
                    NativeParseStatus::Invalid
                },
                Some(error.to_string()),
            ),
        };
        Ok(NativeProvidersList {
            target: target.clone(),
            revision,
            parse_status,
            issue,
            providers: providers.into_values().collect(),
        })
    })
}

fn check_snapshot(
    snapshot: &NativeDocumentSnapshot,
    key: &str,
    expected_revision: &str,
    expected_node: Option<&str>,
) -> AppResult<()> {
    validate_native_key(key)?;
    if snapshot.revision != expected_revision {
        return Err(AppError::new(
            "NATIVE_REVISION_CONFLICT",
            "Native document changed; reload before applying",
        ));
    }
    if snapshot.providers().get(key).map(node_digest).as_deref() != expected_node {
        return Err(AppError::new(
            "NATIVE_NODE_CONFLICT",
            "Native provider changed or its name is already in use",
        ));
    }
    Ok(())
}

fn compensate_error(
    session: &NativeTargetSession<'_>,
    receipt: &NativePatchReceipt,
    error: AppError,
) -> AppError {
    match session.compensate(receipt) {
        Ok(()) => AppError::new(error.code(),"Native archive persistence failed; owned native node changes were compensated"),
        Err(_) => AppError::new("NATIVE_RECOVERY_REQUIRED","Archive persistence and conditional compensation failed; preserve current native file and inspect the private backup")
    }
}

fn save_for_target_checked(
    db: &db::Db,
    target: &NativeTarget,
    input: NativeProviderSaveInput,
    verify_selection: impl FnOnce() -> AppResult<()>,
) -> AppResult<NativeMutationResult> {
    if input.target_id != target.target_id {
        return Err(AppError::new(
            "NATIVE_TARGET_NOT_FOUND",
            "Target identity mismatch",
        ));
    }
    if input.display_name.trim().is_empty()
        || input.display_name.len() > 256
        || input.display_name.chars().any(char::is_control)
    {
        return Err(AppError::new(
            "NATIVE_INVALID_NODE",
            "Display name is invalid",
        ));
    }
    if input.node.is_some() && !input.patch.is_empty() {
        return Err(AppError::new(
            "NATIVE_INVALID_PATCH",
            "Choose raw replacement or field edits, not both",
        ));
    }
    native_cli::with_target_lock(target, |session| {
        verify_selection()?;
        assert_unmanaged(&managed_keys(db, target)?, &input.native_key)?;
        let snapshot = session.snapshot()?;
        check_snapshot(
            &snapshot,
            &input.native_key,
            &input.expected_revision,
            input.expected_node_digest.as_deref(),
        )?;
        if !target.writable {
            return Err(AppError::new(
                "NATIVE_TARGET_READ_ONLY",
                "Native target is read-only",
            ));
        }
        let mut conn = db.open_connection()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|_| {
                AppError::new(
                    "NATIVE_ARCHIVE_FAILED",
                    "Cannot begin native archive transaction",
                )
            })?;
        let previous = profiles::get(&tx, target, &input.native_key)?;
        profiles::check_revision(
            previous.as_ref(),
            input.expected_profile_revision.as_deref(),
        )?;
        let current_node = snapshot.providers().get(&input.native_key);
        let base = current_node.or_else(|| previous.as_ref().map(|p| &p.node));
        let node = match input.node {
            Some(node) => node,
            None => apply_field_patches(
                base.ok_or_else(|| {
                    AppError::new(
                        "NATIVE_INVALID_NODE",
                        "A new provider requires a full native node",
                    )
                })?,
                &input.patch,
            )?,
        };
        validate_provider(target.client, &node)?;
        if !input.apply && current_node.is_some_and(|n| n != &node) {
            return Err(AppError::new(
                "NATIVE_APPLY_REQUIRED",
                "Editing an already joined provider requires applying its native changes",
            ));
        }
        let patches = if input.apply {
            vec![NativeNodePatch {
                native_key: input.native_key.clone(),
                expected_digest: input.expected_node_digest,
                replacement: Some(node.clone()),
            }]
        } else {
            Vec::new()
        };
        let receipt = session.patch(&snapshot.revision, &patches)?;
        let persistence = (|| {
            let profile = profiles::put(
                &tx,
                target,
                &input.native_key,
                input.display_name.trim(),
                &node,
                previous.as_ref(),
            )?;
            tx.commit().map_err(|_| {
                AppError::new("NATIVE_ARCHIVE_FAILED", "Native archive commit failed")
            })?;
            Ok(profile)
        })();
        let profile = persistence.map_err(|error| compensate_error(session, &receipt, error))?;
        let state = if input.apply || current_node.is_some() {
            NativeProviderState::Present
        } else {
            NativeProviderState::Archived
        };
        Ok(NativeMutationResult {
            target_id: target.target_id.clone(),
            revision: receipt.after_revision,
            changed: receipt.changed
                || previous.as_ref().map(|p| p.revision.as_str()) != Some(&profile.revision),
            backup_path: receipt.backup_path,
            provider: Some(summary(
                Some(&profile),
                &input.native_key,
                &node,
                state,
                false,
            )),
        })
    })
}

#[derive(Clone, Copy)]
pub(crate) enum NativeAction {
    Apply,
    Remove,
    Delete { remove_native: bool },
}

fn action_for_target_checked(
    db: &db::Db,
    target: &NativeTarget,
    input: NativeProviderActionInput,
    action: NativeAction,
    verify_selection: impl FnOnce() -> AppResult<()>,
) -> AppResult<NativeMutationResult> {
    if input.target_id != target.target_id {
        return Err(AppError::new(
            "NATIVE_TARGET_NOT_FOUND",
            "Target identity mismatch",
        ));
    }
    native_cli::with_target_lock(target, |session| {
        verify_selection()?;
        assert_unmanaged(&managed_keys(db, target)?, &input.native_key)?;
        let snapshot = session.snapshot()?;
        check_snapshot(
            &snapshot,
            &input.native_key,
            &input.expected_revision,
            input.expected_node_digest.as_deref(),
        )?;
        if !target.writable {
            return Err(AppError::new(
                "NATIVE_TARGET_READ_ONLY",
                "Native target is read-only",
            ));
        }
        let mut conn = db.open_connection()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|_| {
                AppError::new(
                    "NATIVE_ARCHIVE_FAILED",
                    "Cannot begin native archive transaction",
                )
            })?;
        let previous = profiles::get(&tx, target, &input.native_key)?;
        profiles::check_revision(
            previous.as_ref(),
            input.expected_profile_revision.as_deref(),
        )?;
        let current = snapshot.providers().get(&input.native_key);
        let node = current
            .or_else(|| previous.as_ref().map(|p| &p.node))
            .cloned()
            .ok_or_else(|| {
                AppError::new(
                    "NATIVE_PROVIDER_NOT_FOUND",
                    "Native provider or archive no longer exists",
                )
            })?;
        if matches!(
            action,
            NativeAction::Delete {
                remove_native: false
            }
        ) && current.is_some()
        {
            return Err(AppError::new(
                "NATIVE_REMOVE_REQUIRED",
                "Provider is still joined; explicitly choose removal before deleting its archive",
            ));
        }
        let replacement = if matches!(action, NativeAction::Apply) {
            Some(node.clone())
        } else {
            None
        };
        let receipt = session.patch(
            &snapshot.revision,
            &[NativeNodePatch {
                native_key: input.native_key.clone(),
                expected_digest: input.expected_node_digest,
                replacement,
            }],
        )?;
        let persistence = (|| {
            let saved = if matches!(action, NativeAction::Delete { .. }) {
                profiles::delete(&tx, target, &input.native_key)?;
                None
            } else {
                Some(profiles::put(
                    &tx,
                    target,
                    &input.native_key,
                    previous
                        .as_ref()
                        .map(|p| p.display_name.as_str())
                        .unwrap_or(&input.native_key),
                    &node,
                    previous.as_ref(),
                )?)
            };
            tx.commit().map_err(|_| {
                AppError::new("NATIVE_ARCHIVE_FAILED", "Native archive commit failed")
            })?;
            Ok(saved)
        })();
        let saved = persistence.map_err(|error| compensate_error(session, &receipt, error))?;
        let state = if matches!(action, NativeAction::Apply) {
            NativeProviderState::Present
        } else {
            NativeProviderState::Archived
        };
        Ok(NativeMutationResult {
            target_id: target.target_id.clone(),
            revision: receipt.after_revision,
            changed: receipt.changed || matches!(action, NativeAction::Delete { .. }),
            backup_path: receipt.backup_path,
            provider: saved
                .as_ref()
                .map(|p| summary(Some(p), &input.native_key, &node, state, false)),
        })
    })
}

pub(crate) async fn native_cli_providers_list(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    target_id: String,
) -> Result<NativeProvidersList, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("native_cli_providers_list", move || {
        let target = resolve_target(&app, &target_id)?;
        list_for_target_checked(&db, &target, || verify_selected_target(&app, &target))
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn native_cli_provider_read_for_edit(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    target_id: String,
    native_key: String,
) -> Result<NativeProviderEdit, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("native_cli_provider_read_for_edit", move || {
        let target = resolve_target(&app, &target_id)?;
        native_cli::with_target_lock(&target, |session| {
            verify_selected_target(&app, &target)?;
            let managed = managed_keys(&db, &target)?;
            assert_unmanaged(&managed, &native_key)?;
            let snapshot = session.snapshot()?;
            let mut conn = db.open_connection()?;
            let tx = conn
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .map_err(|_| {
                    AppError::new(
                        "NATIVE_ARCHIVE_FAILED",
                        "Cannot refresh editable native profile",
                    )
                })?;
            profiles::sync(&tx, &target, snapshot.providers(), &managed)?;
            let profile = profiles::get(&tx, &target, &native_key)?.ok_or_else(|| {
                AppError::new(
                    "NATIVE_PROVIDER_NOT_FOUND",
                    "Native profile no longer exists",
                )
            })?;
            tx.commit().map_err(|_| {
                AppError::new(
                    "NATIVE_ARCHIVE_FAILED",
                    "Cannot commit native profile refresh",
                )
            })?;
            let state = if snapshot.providers().contains_key(&native_key) {
                NativeProviderState::Present
            } else {
                NativeProviderState::Archived
            };
            let provider = summary(Some(&profile), &native_key, &profile.node, state, false);
            Ok(NativeProviderEdit {
                target: target.clone(),
                revision: snapshot.revision,
                provider,
                node: profile.node,
            })
        })
    })
    .await
    .map_err(Into::into)
}

pub(crate) async fn native_cli_provider_save(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: NativeProviderSaveInput,
) -> Result<NativeMutationResult, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("native_cli_provider_save", move || {
        let target = resolve_target(&app, &input.target_id)?;
        save_for_target_checked(&db, &target, input, || {
            verify_selected_target(&app, &target)
        })
    })
    .await
    .map_err(Into::into)
}
pub(crate) async fn native_cli_provider_action(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: NativeProviderActionInput,
    action: NativeAction,
) -> Result<NativeMutationResult, String> {
    let db = ensure_db_ready(app.clone(), db_state.inner()).await?;
    blocking::run("native_cli_provider_action", move || {
        let target = resolve_target(&app, &input.target_id)?;
        action_for_target_checked(&db, &target, input, action, || {
            verify_selected_target(&app, &target)
        })
    })
    .await
    .map_err(Into::into)
}

#[cfg(test)]
fn list_for_target(db: &db::Db, target: &NativeTarget) -> AppResult<NativeProvidersList> {
    list_for_target_checked(db, target, || Ok(()))
}
#[cfg(test)]
fn save_for_target(
    db: &db::Db,
    target: &NativeTarget,
    input: NativeProviderSaveInput,
) -> AppResult<NativeMutationResult> {
    save_for_target_checked(db, target, input, || Ok(()))
}
#[cfg(test)]
fn action_for_target(
    db: &db::Db,
    target: &NativeTarget,
    input: NativeProviderActionInput,
    action: NativeAction,
) -> AppResult<NativeMutationResult> {
    action_for_target_checked(db, target, input, action, || Ok(()))
}

#[cfg(test)]
#[path = "../infra/native_cli/service_tests.rs"]
mod tests;
