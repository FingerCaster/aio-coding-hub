//! Thin IPC wrappers for native model configuration.
use crate::app::native_cli_service::{self, NativeAction};
use crate::app_state::DbInitState;
use crate::domain::native_cli::*;

#[tauri::command]
#[specta::specta]
pub(crate) async fn native_cli_targets_list(
    app: tauri::AppHandle,
    client: NativeClient,
) -> Result<Vec<NativeTarget>, String> {
    native_cli_service::native_cli_targets_list(app, client).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn native_cli_target_validate(
    app: tauri::AppHandle,
    selection: NativeTargetSelection,
) -> Result<NativeTarget, String> {
    native_cli_service::native_cli_target_validate(app, selection).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn native_cli_target_select(
    app: tauri::AppHandle,
    selection: NativeTargetSelection,
) -> Result<NativeTarget, String> {
    native_cli_service::native_cli_target_select(app, selection).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn native_cli_providers_list(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    target_id: String,
) -> Result<NativeProvidersList, String> {
    native_cli_service::native_cli_providers_list(app, db_state, target_id).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn native_cli_provider_read_for_edit(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    target_id: String,
    native_key: String,
) -> Result<NativeProviderEdit, String> {
    native_cli_service::native_cli_provider_read_for_edit(app, db_state, target_id, native_key)
        .await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn native_cli_provider_save(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: NativeProviderSaveInput,
) -> Result<NativeMutationResult, String> {
    native_cli_service::native_cli_provider_save(app, db_state, input).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn native_cli_provider_apply(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: NativeProviderActionInput,
) -> Result<NativeMutationResult, String> {
    native_cli_service::native_cli_provider_action(app, db_state, input, NativeAction::Apply).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn native_cli_provider_remove(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: NativeProviderActionInput,
) -> Result<NativeMutationResult, String> {
    native_cli_service::native_cli_provider_action(app, db_state, input, NativeAction::Remove).await
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn native_cli_provider_delete(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, DbInitState>,
    input: NativeProviderDeleteInput,
) -> Result<NativeMutationResult, String> {
    let remove_native = input.remove_from_native;
    let action = NativeProviderActionInput {
        target_id: input.target_id,
        native_key: input.native_key,
        expected_revision: input.expected_revision,
        expected_node_digest: input.expected_node_digest,
        expected_profile_revision: input.expected_profile_revision,
    };
    native_cli_service::native_cli_provider_action(
        app,
        db_state,
        action,
        NativeAction::Delete { remove_native },
    )
    .await
}
