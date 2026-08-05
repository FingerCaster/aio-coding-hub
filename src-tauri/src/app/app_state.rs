//! Usage: Shared DB initialization gate used by `commands/*`.

use crate::shared::error::AppResult;
use crate::{blocking, db};
use std::future::Future;
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};

#[derive(Default)]
pub(crate) struct DbInitState(pub(crate) AsyncMutex<Option<db::Db>>);

pub(super) async fn ensure_db_ready_with<F, Fut>(
    state: &DbInitState,
    initialize: F,
) -> AppResult<db::Db>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = AppResult<db::Db>>,
{
    let mut guard = state.0.lock().await;
    if let Some(db) = guard.as_ref() {
        return Ok(db.clone());
    }

    let db = initialize().await?;
    *guard = Some(db.clone());
    Ok(db)
}

pub(crate) async fn ensure_db_ready<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: &DbInitState,
) -> AppResult<db::Db> {
    ensure_db_ready_with(state, || blocking::run("db_init", move || db::init(&app))).await
}

pub(crate) async fn prepare_db_reset<'a>(state: &'a DbInitState) -> MutexGuard<'a, Option<db::Db>> {
    let mut guard = state.0.lock().await;
    // Hold the cache lock through file deletion so no concurrent command can
    // recreate the pool midway through a destructive reset.
    let _ = guard.take();
    guard
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::error::AppError;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn failed_initialization_is_retried_and_success_is_cached() {
        let temp = tempfile::tempdir().expect("tempdir");
        let initialized_db =
            db::init_for_tests(&temp.path().join("retry.db")).expect("initialize test db");
        let state = DbInitState::default();
        let attempts = AtomicUsize::new(0);

        let first = ensure_db_ready_with(&state, || async {
            attempts.fetch_add(1, Ordering::SeqCst);
            Err(AppError::new(
                "DB_ERROR",
                "transient initialization failure",
            ))
        })
        .await;
        assert!(first.is_err());

        let second = ensure_db_ready_with(&state, || async {
            attempts.fetch_add(1, Ordering::SeqCst);
            Ok(initialized_db.clone())
        })
        .await;
        assert!(second.is_ok());

        let cached = ensure_db_ready_with(&state, || async {
            attempts.fetch_add(1, Ordering::SeqCst);
            Err(AppError::new("DB_ERROR", "cached value was not reused"))
        })
        .await;
        assert!(cached.is_ok());
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn concurrent_successful_initialization_runs_once() {
        let temp = tempfile::tempdir().expect("tempdir");
        let initialized_db =
            db::init_for_tests(&temp.path().join("concurrent.db")).expect("initialize test db");
        let state = DbInitState::default();
        let attempts = AtomicUsize::new(0);

        let first = ensure_db_ready_with(&state, || async {
            attempts.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            Ok(initialized_db.clone())
        });
        let second = ensure_db_ready_with(&state, || async {
            attempts.fetch_add(1, Ordering::SeqCst);
            Ok(initialized_db.clone())
        });

        let (first, second) = tokio::join!(first, second);
        assert!(first.is_ok());
        assert!(second.is_ok());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }
}
