//! Usage: Persist provider circuit breaker state to sqlite (buffered writer + load helpers).

use crate::shared::error::db_err;
use crate::shared::time::now_unix_seconds;
use crate::{circuit_breaker, db};
use rusqlite::{params, ErrorCode, TransactionBehavior};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;

const WRITE_BUFFER_CAPACITY: usize = 512;
const WRITE_BATCH_MAX: usize = 200;
const INSERT_RETRY_MAX_ATTEMPTS: u32 = 6;
const INSERT_RETRY_BASE_DELAY_MS: u64 = 20;
const INSERT_RETRY_MAX_DELAY_MS: u64 = 400;
const FAILURE_TIMESTAMPS_JSON_MAX_BYTES: usize = circuit_breaker::MAX_FAILURE_TIMESTAMPS * 24 + 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DbWriteErrorKind {
    Busy,
    Other,
}

#[derive(Debug)]
struct DbWriteError {
    kind: DbWriteErrorKind,
    message: String,
}

impl DbWriteError {
    fn other(message: String) -> Self {
        Self {
            kind: DbWriteErrorKind::Other,
            message,
        }
    }

    fn from_rusqlite(context: &'static str, err: rusqlite::Error) -> Self {
        let kind = classify_rusqlite_error(&err);
        Self {
            kind,
            message: format!("DB_ERROR: {context}: {err}"),
        }
    }

    fn is_retryable(&self) -> bool {
        self.kind == DbWriteErrorKind::Busy
    }
}

fn classify_rusqlite_error(err: &rusqlite::Error) -> DbWriteErrorKind {
    match err {
        rusqlite::Error::SqliteFailure(e, _) => match e.code {
            ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => DbWriteErrorKind::Busy,
            _ => DbWriteErrorKind::Other,
        },
        _ => DbWriteErrorKind::Other,
    }
}

fn retry_delay(attempt_index: u32) -> Duration {
    let exp = attempt_index.min(20);
    let raw = INSERT_RETRY_BASE_DELAY_MS.saturating_mul(1u64.checked_shl(exp).unwrap_or(u64::MAX));
    Duration::from_millis(raw.min(INSERT_RETRY_MAX_DELAY_MS))
}

fn bounded_failure_timestamp_slice(timestamps: &[u64]) -> &[u64] {
    let start = timestamps
        .len()
        .saturating_sub(circuit_breaker::MAX_FAILURE_TIMESTAMPS);
    &timestamps[start..]
}

fn serialize_failure_timestamps(timestamps: &[u64]) -> String {
    serde_json::to_string(bounded_failure_timestamp_slice(timestamps))
        .unwrap_or_else(|_| "[]".to_string())
}

fn is_inert_closed_state(item: &circuit_breaker::CircuitPersistedState) -> bool {
    item.provider_id > 0
        && item.state_revision == 0
        && item.state == circuit_breaker::CircuitState::Closed
        && item.failure_timestamps.is_empty()
        && item.half_open_success_count == 0
        && item.open_until.is_none()
        && item.probe_reference_at.is_none()
        && item.next_probe_at.is_none()
        && item.natural_probe_due_at.is_none()
        && item.recovery_guard_until.is_none()
}

fn deserialize_failure_timestamps(raw: &str) -> Vec<u64> {
    if raw.len() > FAILURE_TIMESTAMPS_JSON_MAX_BYTES {
        tracing::warn!(
            bytes = raw.len(),
            max_bytes = FAILURE_TIMESTAMPS_JSON_MAX_BYTES,
            "ignoring oversized circuit breaker failure timestamp history"
        );
        return Vec::new();
    }

    let mut timestamps: Vec<u64> = serde_json::from_str(raw).unwrap_or_default();
    if timestamps.len() > circuit_breaker::MAX_FAILURE_TIMESTAMPS {
        let excess = timestamps.len() - circuit_breaker::MAX_FAILURE_TIMESTAMPS;
        timestamps.drain(..excess);
    }
    timestamps
}

pub fn start_buffered_writer(
    db: db::Db,
) -> (
    mpsc::Sender<circuit_breaker::CircuitPersistedState>,
    tauri::async_runtime::JoinHandle<()>,
) {
    let (tx, rx) = mpsc::channel::<circuit_breaker::CircuitPersistedState>(WRITE_BUFFER_CAPACITY);
    let task = tauri::async_runtime::spawn_blocking(move || {
        writer_loop(db, rx);
    });
    (tx, task)
}

pub fn upsert_durable(
    db: &db::Db,
    item: &circuit_breaker::CircuitPersistedState,
) -> Result<(), String> {
    upsert_many_durable(db, std::slice::from_ref(item))
}

pub fn upsert_many_durable(
    db: &db::Db,
    items: &[circuit_breaker::CircuitPersistedState],
) -> Result<(), String> {
    insert_batch_with_retries(db, items).map_err(|err| err.message)
}

fn writer_loop(db: db::Db, mut rx: mpsc::Receiver<circuit_breaker::CircuitPersistedState>) {
    let mut buffer: Vec<circuit_breaker::CircuitPersistedState> =
        Vec::with_capacity(WRITE_BATCH_MAX);

    while let Some(item) = rx.blocking_recv() {
        buffer.push(item);

        while buffer.len() < WRITE_BATCH_MAX {
            match rx.try_recv() {
                Ok(next) => buffer.push(next),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
            }
        }

        if let Err(err) = insert_batch_with_retries(&db, &buffer) {
            tracing::error!(error = %err.message, "circuit breaker state batch insert failed");
        }
        buffer.clear();
    }

    if !buffer.is_empty() {
        if let Err(err) = insert_batch_with_retries(&db, &buffer) {
            tracing::error!(error = %err.message, "circuit breaker state final batch insert failed");
        }
    }
}

fn insert_batch_with_retries(
    db: &db::Db,
    items: &[circuit_breaker::CircuitPersistedState],
) -> Result<(), DbWriteError> {
    if items.is_empty() {
        return Ok(());
    }

    let mut attempt: u32 = 0;
    loop {
        match insert_batch_once(db, items) {
            Ok(()) => return Ok(()),
            Err(err) => {
                attempt = attempt.saturating_add(1);
                if !err.is_retryable() || attempt >= INSERT_RETRY_MAX_ATTEMPTS {
                    return Err(err);
                }
                let delay = retry_delay(attempt.saturating_sub(1));
                tracing::debug!(
                    attempt = attempt,
                    delay_ms = delay.as_millis(),
                    error = %err.message,
                    "sqlite busy/locked; retrying provider_circuit_breakers insert"
                );
                std::thread::sleep(delay);
            }
        }
    }
}

fn insert_batch_once(
    db: &db::Db,
    items: &[circuit_breaker::CircuitPersistedState],
) -> Result<(), DbWriteError> {
    let mut latest_by_provider: HashMap<i64, circuit_breaker::CircuitPersistedState> =
        HashMap::with_capacity(items.len().min(WRITE_BATCH_MAX));
    for item in items {
        let replace = latest_by_provider
            .get(&item.provider_id)
            .is_none_or(|current| {
                (item.state_revision, item.updated_at)
                    >= (current.state_revision, current.updated_at)
            });
        if replace {
            latest_by_provider.insert(item.provider_id, item.clone());
        }
    }

    let mut conn = db
        .open_connection()
        .map_err(|e| DbWriteError::other(e.to_string()))?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| DbWriteError::from_rusqlite("failed to start transaction", e))?;

    {
        let mut stmt = tx
            .prepare_cached(
                r#"
INSERT INTO provider_circuit_breakers (
  provider_id,
  state,
  failure_count,
  failure_timestamps_json,
  half_open_success_count,
  open_until,
  probe_reference_at,
  next_probe_at,
  natural_probe_due_at,
  recovery_guard_until,
  state_revision,
  updated_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
ON CONFLICT(provider_id) DO UPDATE SET
  state = excluded.state,
  failure_count = excluded.failure_count,
  failure_timestamps_json = excluded.failure_timestamps_json,
  half_open_success_count = excluded.half_open_success_count,
  open_until = excluded.open_until,
  probe_reference_at = excluded.probe_reference_at,
  next_probe_at = excluded.next_probe_at,
  natural_probe_due_at = excluded.natural_probe_due_at,
  recovery_guard_until = excluded.recovery_guard_until,
  state_revision = excluded.state_revision,
  updated_at = excluded.updated_at
WHERE excluded.state_revision > provider_circuit_breakers.state_revision
   OR (
     excluded.state_revision = provider_circuit_breakers.state_revision
     AND excluded.updated_at >= provider_circuit_breakers.updated_at
   )
"#,
            )
            .map_err(|e| {
                DbWriteError::from_rusqlite("failed to prepare circuit breaker upsert", e)
            })?;

        for item in latest_by_provider.values() {
            let updated_at = if item.updated_at > 0 {
                item.updated_at
            } else {
                now_unix_seconds()
            };

            let bounded_timestamps = bounded_failure_timestamp_slice(&item.failure_timestamps);
            let timestamps_json = serialize_failure_timestamps(bounded_timestamps);
            let failure_count = bounded_timestamps.len().min(u32::MAX as usize) as i64;

            stmt.execute(params![
                item.provider_id,
                item.state.as_str(),
                failure_count,
                timestamps_json,
                item.half_open_success_count as i64,
                item.open_until,
                item.probe_reference_at,
                item.next_probe_at,
                item.natural_probe_due_at,
                item.recovery_guard_until,
                item.state_revision.min(i64::MAX as u64) as i64,
                updated_at
            ])
            .map_err(|e| {
                DbWriteError::from_rusqlite("failed to upsert provider_circuit_breaker", e)
            })?;
        }
    }

    tx.commit()
        .map_err(|e| DbWriteError::from_rusqlite("failed to commit transaction", e))?;

    Ok(())
}

pub fn load_all(
    db: &db::Db,
) -> crate::shared::error::AppResult<HashMap<i64, circuit_breaker::CircuitPersistedState>> {
    let conn = db.open_connection()?;
    let mut stmt = conn
        .prepare_cached(
            r#"
    SELECT
      provider_id,
      state,
      failure_timestamps_json,
      half_open_success_count,
      open_until,
      probe_reference_at,
      next_probe_at,
      natural_probe_due_at,
      recovery_guard_until,
      state_revision,
      updated_at
    FROM provider_circuit_breakers
    "#,
        )
        .map_err(|e| db_err!("failed to prepare circuit breaker load query: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            let raw_state: String = row.get("state")?;
            let open_until: Option<i64> = row.get("open_until")?;
            let timestamps_json: String = row
                .get::<_, String>("failure_timestamps_json")
                .unwrap_or_else(|_| "[]".to_string());
            let half_open_success_count: i64 =
                row.get::<_, i64>("half_open_success_count").unwrap_or(0);
            Ok(circuit_breaker::CircuitPersistedState {
                provider_id: row.get("provider_id")?,
                state: circuit_breaker::CircuitState::from_str(&raw_state),
                failure_timestamps: deserialize_failure_timestamps(&timestamps_json),
                half_open_success_count: half_open_success_count.max(0).min(u32::MAX as i64) as u32,
                open_until,
                probe_reference_at: row.get("probe_reference_at").ok().flatten(),
                next_probe_at: row.get("next_probe_at").ok().flatten(),
                natural_probe_due_at: row.get("natural_probe_due_at").ok().flatten(),
                recovery_guard_until: row.get("recovery_guard_until").ok().flatten(),
                state_revision: row.get::<_, i64>("state_revision").unwrap_or(0).max(0) as u64,
                updated_at: row.get("updated_at")?,
            })
        })
        .map_err(|e| db_err!("failed to query circuit breaker states: {e}"))?;

    let mut items = HashMap::new();
    for row in rows {
        let item = row.map_err(|e| db_err!("failed to read circuit breaker state: {e}"))?;
        if is_inert_closed_state(&item) {
            continue;
        }
        items.insert(item.provider_id, item);
    }

    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_test_db() -> (tempfile::TempDir, db::Db) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("provider_circuit_breakers.db");
        let db = db::init_for_tests(&db_path).expect("init db");
        (dir, db)
    }

    fn insert_test_provider(db: &db::Db, provider_id: i64) {
        let conn = db.open_connection().expect("open db");
        conn.execute(
            r#"
            INSERT INTO providers(
              id,
              provider_uuid,
              cli_key,
              name,
              base_url,
              api_key_plaintext,
              enabled,
              priority,
              created_at,
              updated_at,
              sort_order,
              cost_multiplier,
              base_urls_json,
              base_url_mode,
              supported_models_json,
              model_mapping_json
            ) VALUES (?1, ?2, 'test-cli', ?3, 'https://example.test', '', 1, 100, 1, 1, 0, 1.0, '[]', 'order', '{}', '{}')
            "#,
            params![
                provider_id,
                format!("00000000-0000-4000-8000-{provider_id:012x}"),
                format!("provider-{provider_id}")
            ],
        )
        .expect("insert provider");
    }

    fn oversized_timestamps() -> Vec<u64> {
        (0..(circuit_breaker::MAX_FAILURE_TIMESTAMPS + 3))
            .map(|value| value as u64)
            .collect()
    }

    #[test]
    fn deserialize_failure_timestamps_keeps_most_recent_capped_entries() {
        let raw = serde_json::to_string(&oversized_timestamps()).expect("serialize timestamps");

        let timestamps = deserialize_failure_timestamps(&raw);

        assert_eq!(timestamps.len(), circuit_breaker::MAX_FAILURE_TIMESTAMPS);
        assert_eq!(timestamps.first().copied(), Some(3));
        assert_eq!(
            timestamps.last().copied(),
            Some((circuit_breaker::MAX_FAILURE_TIMESTAMPS + 2) as u64)
        );
    }

    #[test]
    fn deserialize_failure_timestamps_rejects_oversized_json_before_parse() {
        let raw = format!("[{}]", "1,".repeat(FAILURE_TIMESTAMPS_JSON_MAX_BYTES));
        assert!(raw.len() > FAILURE_TIMESTAMPS_JSON_MAX_BYTES);

        let timestamps = deserialize_failure_timestamps(&raw);

        assert!(timestamps.is_empty());
    }

    #[test]
    fn serialize_failure_timestamps_writes_only_capped_entries() {
        let raw = serialize_failure_timestamps(&oversized_timestamps());
        let timestamps: Vec<u64> = serde_json::from_str(&raw).expect("parse serialized timestamps");

        assert_eq!(timestamps.len(), circuit_breaker::MAX_FAILURE_TIMESTAMPS);
        assert_eq!(timestamps.first().copied(), Some(3));
    }

    #[test]
    fn load_all_skips_legacy_inert_closed_rows() {
        let (_dir, db) = init_test_db();
        insert_test_provider(&db, 1);
        insert_test_provider(&db, 2);

        let conn = db.open_connection().expect("open db");
        conn.execute(
            r#"
            INSERT INTO provider_circuit_breakers(
              provider_id,
              state,
              failure_count,
              failure_timestamps_json,
              half_open_success_count,
              open_until,
              updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                1_i64,
                "CLOSED",
                0_i64,
                "[]",
                0_i64,
                Option::<i64>::None,
                1_i64
            ],
        )
        .expect("insert inert closed row");
        conn.execute(
            r#"
            INSERT INTO provider_circuit_breakers(
              provider_id,
              state,
              failure_count,
              failure_timestamps_json,
              half_open_success_count,
              open_until,
              updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                2_i64,
                "CLOSED",
                1_i64,
                "[42]",
                0_i64,
                Option::<i64>::None,
                1_i64
            ],
        )
        .expect("insert active closed row");
        drop(conn);

        let loaded = load_all(&db).expect("load circuit states");

        assert!(!loaded.contains_key(&1));
        assert!(loaded.contains_key(&2));
    }

    fn persisted_state(
        provider_id: i64,
        state: circuit_breaker::CircuitState,
        state_revision: u64,
        updated_at: i64,
    ) -> circuit_breaker::CircuitPersistedState {
        circuit_breaker::CircuitPersistedState {
            provider_id,
            state,
            failure_timestamps: if state == circuit_breaker::CircuitState::Open {
                vec![10]
            } else {
                Vec::new()
            },
            half_open_success_count: 0,
            open_until: (state == circuit_breaker::CircuitState::Open).then_some(100),
            probe_reference_at: None,
            next_probe_at: None,
            natural_probe_due_at: None,
            recovery_guard_until: None,
            state_revision,
            updated_at,
        }
    }

    #[test]
    fn insert_batch_persists_revisioned_closed_tombstone() {
        let (_dir, db) = init_test_db();
        insert_test_provider(&db, 7);

        let open = persisted_state(7, circuit_breaker::CircuitState::Open, 1, 10);
        upsert_durable(&db, &open).expect("insert open state");
        assert!(load_all(&db).expect("load open state").contains_key(&7));

        let tombstone = persisted_state(7, circuit_breaker::CircuitState::Closed, 2, 20);
        upsert_durable(&db, &tombstone).expect("upsert closed tombstone");

        let loaded = load_all(&db).expect("load tombstone");
        let loaded = loaded.get(&7).expect("revisioned tombstone retained");
        assert_eq!(loaded.state, circuit_breaker::CircuitState::Closed);
        assert_eq!(loaded.state_revision, 2);

        let conn = db.open_connection().expect("open db");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM provider_circuit_breakers WHERE provider_id = ?1",
                params![7_i64],
                |row| row.get(0),
            )
            .expect("count circuit rows");
        assert_eq!(count, 1);
    }

    #[test]
    fn upsert_durable_applies_new_revision_and_ignores_old_revision() {
        let (_dir, db) = init_test_db();
        insert_test_provider(&db, 11);

        let open = persisted_state(11, circuit_breaker::CircuitState::Open, 3, 30);
        upsert_durable(&db, &open).expect("insert open revision");

        let closed = persisted_state(11, circuit_breaker::CircuitState::Closed, 4, 40);
        upsert_durable(&db, &closed).expect("apply newer closed revision");

        let stale_open = persisted_state(11, circuit_breaker::CircuitState::Open, 3, 999);
        upsert_durable(&db, &stale_open).expect("older revision is a successful no-op");

        let loaded = load_all(&db).expect("load durable state");
        let state = loaded.get(&11).expect("provider state");
        assert_eq!(state.state, circuit_breaker::CircuitState::Closed);
        assert_eq!(state.state_revision, 4);
        assert_eq!(state.updated_at, 40);
    }

    #[test]
    fn upsert_durable_returns_database_error() {
        let (_dir, db) = init_test_db();
        let missing_provider = persisted_state(999, circuit_breaker::CircuitState::Open, 1, 10);

        let err = upsert_durable(&db, &missing_provider)
            .expect_err("foreign-key failure must be returned to durable caller");

        assert!(err.starts_with("DB_ERROR:"), "unexpected error: {err}");
    }

    #[test]
    fn upsert_many_durable_persists_multiple_providers_in_one_batch() {
        let (_dir, db) = init_test_db();
        insert_test_provider(&db, 12);
        insert_test_provider(&db, 13);
        let states = [
            persisted_state(12, circuit_breaker::CircuitState::Closed, 4, 40),
            persisted_state(13, circuit_breaker::CircuitState::Open, 7, 70),
        ];

        upsert_many_durable(&db, &states).expect("persist provider batch");

        let loaded = load_all(&db).expect("load provider batch");
        assert_eq!(
            loaded.get(&12).expect("provider 12").state,
            circuit_breaker::CircuitState::Closed
        );
        assert_eq!(loaded.get(&12).expect("provider 12").state_revision, 4);
        assert_eq!(
            loaded.get(&13).expect("provider 13").state,
            circuit_breaker::CircuitState::Open
        );
        assert_eq!(loaded.get(&13).expect("provider 13").state_revision, 7);
    }

    #[test]
    fn upsert_many_durable_rolls_back_entire_batch_on_database_error() {
        let (_dir, db) = init_test_db();
        insert_test_provider(&db, 14);
        insert_test_provider(&db, 15);
        let conn = db.open_connection().expect("open db");
        conn.execute_batch(
            r#"
            CREATE TRIGGER fail_second_circuit_insert
            BEFORE INSERT ON provider_circuit_breakers
            WHEN (SELECT COUNT(*) FROM provider_circuit_breakers) >= 1
            BEGIN
              SELECT RAISE(ABORT, 'forced second circuit insert failure');
            END;
            "#,
        )
        .expect("create failure trigger");
        drop(conn);
        let states = [
            persisted_state(14, circuit_breaker::CircuitState::Closed, 2, 20),
            persisted_state(15, circuit_breaker::CircuitState::Open, 1, 10),
        ];

        upsert_many_durable(&db, &states)
            .expect_err("second insert failure must roll back the whole batch");

        let loaded = load_all(&db).expect("load after failed batch");
        assert!(!loaded.contains_key(&14));
        assert!(!loaded.contains_key(&15));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn queued_open_then_reset_then_late_open_reloads_closed() {
        let (_dir, db) = init_test_db();
        let provider_id = 10;
        insert_test_provider(&db, provider_id);

        let mut queued_open =
            persisted_state(provider_id, circuit_breaker::CircuitState::Open, 7, 70);
        queued_open.probe_reference_at = Some(70);
        queued_open.next_probe_at = Some(100);
        queued_open.natural_probe_due_at = Some(370);

        let (tx, writer_task) = start_buffered_writer(db.clone());
        tx.send(queued_open.clone())
            .await
            .expect("queue old open state");

        let circuit = circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig::default(),
            HashMap::from([(provider_id, queued_open.clone())]),
            Some(tx.clone()),
        );
        let reset = circuit.reset(provider_id, 80);
        assert_eq!(reset.state, circuit_breaker::CircuitState::Closed);
        assert!(reset.state_revision > queued_open.state_revision);

        tx.send(queued_open)
            .await
            .expect("deliver stale open after reset");
        drop(circuit);
        drop(tx);
        writer_task.await.expect("join circuit writer");

        let loaded = load_all(&db).expect("reload persisted circuit states");
        let tombstone = loaded.get(&provider_id).expect("closed tombstone retained");
        assert_eq!(tombstone.state, circuit_breaker::CircuitState::Closed);
        assert_eq!(tombstone.state_revision, reset.state_revision);

        let reloaded = circuit_breaker::CircuitBreaker::new(
            circuit_breaker::CircuitBreakerConfig::default(),
            loaded,
            None,
        );
        assert_eq!(
            reloaded.snapshot(provider_id, 81).state,
            circuit_breaker::CircuitState::Closed
        );
    }

    #[test]
    fn batch_dedup_and_sql_guard_reject_out_of_order_open_resurrection() {
        let (_dir, db) = init_test_db();
        insert_test_provider(&db, 8);

        let tombstone = persisted_state(8, circuit_breaker::CircuitState::Closed, 8, 80);
        let stale_open = persisted_state(8, circuit_breaker::CircuitState::Open, 7, 999);
        insert_batch_once(&db, &[tombstone.clone(), stale_open.clone()])
            .expect("same-batch upsert");
        let loaded = load_all(&db).expect("load same-batch state");
        assert_eq!(
            loaded.get(&8).expect("state").state,
            circuit_breaker::CircuitState::Closed
        );

        insert_batch_once(&db, &[stale_open]).expect("cross-batch stale upsert");
        let loaded = load_all(&db).expect("load cross-batch state");
        let state = loaded.get(&8).expect("state");
        assert_eq!(state.state, circuit_breaker::CircuitState::Closed);
        assert_eq!(state.state_revision, 8);
    }

    #[test]
    fn equal_revision_uses_updated_at_as_tie_breaker() {
        let (_dir, db) = init_test_db();
        insert_test_provider(&db, 9);

        let tombstone = persisted_state(9, circuit_breaker::CircuitState::Closed, 5, 50);
        let stale_open = persisted_state(9, circuit_breaker::CircuitState::Open, 5, 49);
        insert_batch_once(&db, &[tombstone]).expect("insert tombstone");
        insert_batch_once(&db, &[stale_open]).expect("ignore stale equal revision");

        let loaded = load_all(&db).expect("load state");
        assert_eq!(
            loaded.get(&9).expect("state").state,
            circuit_breaker::CircuitState::Closed
        );
    }
}
