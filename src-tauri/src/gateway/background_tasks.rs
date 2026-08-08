//! Usage: Gateway background writers and refresh-loop ownership.

use crate::shared::blocking;
use crate::{circuit_breaker, db, provider_circuit_breakers, request_logs};
use std::time::Duration;
use tauri::Manager;
use tokio::sync::{mpsc, oneshot, watch};

const ACCOUNT_USAGE_GATEWAY_HEARTBEAT: Duration = Duration::from_secs(5);

pub(super) type GatewayBackgroundTaskHandles = (
    tauri::async_runtime::JoinHandle<()>,
    tauri::async_runtime::JoinHandle<()>,
    watch::Sender<bool>,
    tauri::async_runtime::JoinHandle<()>,
    watch::Sender<bool>,
    tauri::async_runtime::JoinHandle<()>,
);

pub(super) struct GatewayBackgroundTasks {
    log_tx: mpsc::Sender<request_logs::RequestLogInsert>,
    circuit_persist_tx: mpsc::Sender<circuit_breaker::CircuitPersistedState>,
    log_task: tauri::async_runtime::JoinHandle<()>,
    circuit_task: tauri::async_runtime::JoinHandle<()>,
    oauth_refresh_shutdown: watch::Sender<bool>,
    oauth_refresh_task: tauri::async_runtime::JoinHandle<()>,
    account_usage_shutdown: watch::Sender<bool>,
    account_usage_reconcile_tx: mpsc::Sender<oneshot::Sender<Result<(), String>>>,
    account_usage_task: tauri::async_runtime::JoinHandle<()>,
}

pub(crate) async fn reconcile_account_usage_gateway_targets<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    db: &db::Db,
) -> Result<(), String> {
    let db_for_query = db.clone();
    let contexts = blocking::run("account_usage_gateway_targets", move || {
        let connection = db_for_query.open_connection()?;
        crate::providers::list_account_usage_gateway_target_contexts(&connection)
    })
    .await
    .map_err(Into::<String>::into)?;
    let targets = contexts
        .into_iter()
        .filter_map(|(provider_id, context)| {
            crate::app::provider_account_usage_runtime::ProviderAccountUsageTarget::from_gateway_fetch_context(
                provider_id,
                &context,
            )
        })
        .collect();
    let runtime = app
        .try_state::<crate::app::provider_account_usage_runtime::ProviderAccountUsageRuntimeState>()
        .ok_or_else(|| "SYSTEM_ERROR: account usage runtime state is unavailable".to_string())?;
    runtime.replace_gateway_targets(app, targets).await
}

fn spawn_account_usage_gateway_coordinator<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    db: db::Db,
    mut shutdown: watch::Receiver<bool>,
    mut reconcile_rx: mpsc::Receiver<oneshot::Sender<Result<(), String>>>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = reconcile_account_usage_gateway_targets(&app, &db).await {
            tracing::warn!(
                error = %error,
                "failed to reconcile account usage gateway targets"
            );
        }
        loop {
            if *shutdown.borrow() {
                break;
            }
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                request = reconcile_rx.recv() => {
                    let Some(reply) = request else {
                        break;
                    };
                    let result = reconcile_account_usage_gateway_targets(&app, &db).await;
                    let _ = reply.send(result);
                }
                _ = tokio::time::sleep(ACCOUNT_USAGE_GATEWAY_HEARTBEAT) => {
                    if let Err(error) = reconcile_account_usage_gateway_targets(&app, &db).await {
                        tracing::warn!(
                            error = %error,
                            "failed to reconcile account usage gateway targets"
                        );
                    }
                }
            }
        }
        if let Some(runtime) = app.try_state::<
            crate::app::provider_account_usage_runtime::ProviderAccountUsageRuntimeState,
        >() {
            runtime.release_gateway_targets().await;
        }
    })
}

impl GatewayBackgroundTasks {
    pub(super) fn start<R: tauri::Runtime>(app: tauri::AppHandle<R>, db: db::Db) -> Self {
        let (log_tx, log_task) = request_logs::start_buffered_writer(app.clone(), db.clone());
        let (circuit_persist_tx, circuit_task) =
            provider_circuit_breakers::start_buffered_writer(db.clone());
        let (oauth_refresh_shutdown, oauth_refresh_rx) = watch::channel(false);
        let oauth_refresh_task = super::oauth::refresh_loop::spawn(db.clone(), oauth_refresh_rx);
        let (account_usage_shutdown, account_usage_rx) = watch::channel(false);
        let (account_usage_reconcile_tx, account_usage_reconcile_rx) = mpsc::channel(8);
        let account_usage_task = spawn_account_usage_gateway_coordinator(
            app,
            db,
            account_usage_rx,
            account_usage_reconcile_rx,
        );

        Self {
            log_tx,
            circuit_persist_tx,
            log_task,
            circuit_task,
            oauth_refresh_shutdown,
            oauth_refresh_task,
            account_usage_shutdown,
            account_usage_reconcile_tx,
            account_usage_task,
        }
    }

    pub(super) fn log_tx(&self) -> mpsc::Sender<request_logs::RequestLogInsert> {
        self.log_tx.clone()
    }

    pub(super) fn circuit_persist_tx(
        &self,
    ) -> mpsc::Sender<circuit_breaker::CircuitPersistedState> {
        self.circuit_persist_tx.clone()
    }

    pub(super) fn account_usage_reconcile_tx(
        &self,
    ) -> mpsc::Sender<oneshot::Sender<Result<(), String>>> {
        self.account_usage_reconcile_tx.clone()
    }

    pub(super) fn into_handles(self) -> GatewayBackgroundTaskHandles {
        let _ = self.oauth_refresh_shutdown.send(true);
        let _ = self.account_usage_shutdown.send(true);
        (
            self.log_task,
            self.circuit_task,
            self.oauth_refresh_shutdown,
            self.oauth_refresh_task,
            self.account_usage_shutdown,
            self.account_usage_task,
        )
    }

    #[cfg(test)]
    pub(super) fn for_tests(rt: &tokio::runtime::Runtime) -> Self {
        let (log_tx, _log_rx) = mpsc::channel(1);
        let (circuit_persist_tx, _circuit_rx) = mpsc::channel(1);
        let (oauth_refresh_shutdown, _oauth_refresh_rx) = watch::channel(false);
        let (account_usage_shutdown, _account_usage_rx) = watch::channel(false);
        let (account_usage_reconcile_tx, mut account_usage_reconcile_rx) =
            mpsc::channel::<oneshot::Sender<Result<(), String>>>(1);
        let account_usage_task = rt.spawn(async move {
            while let Some(reply) = account_usage_reconcile_rx.recv().await {
                let _ = reply.send(Ok(()));
            }
        });

        Self {
            log_tx,
            circuit_persist_tx,
            log_task: tauri::async_runtime::JoinHandle::Tokio(rt.spawn(async {})),
            circuit_task: tauri::async_runtime::JoinHandle::Tokio(rt.spawn(async {})),
            oauth_refresh_shutdown,
            oauth_refresh_task: tauri::async_runtime::JoinHandle::Tokio(rt.spawn(async {})),
            account_usage_shutdown,
            account_usage_reconcile_tx,
            account_usage_task: tauri::async_runtime::JoinHandle::Tokio(account_usage_task),
        }
    }
}
