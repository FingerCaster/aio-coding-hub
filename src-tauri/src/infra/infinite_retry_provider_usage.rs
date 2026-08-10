//! Bounded per-Provider usage attribution for Codex infinite-retry requests.

use crate::db;
use crate::gateway::infinite_retry::ProviderBucketSnapshot;
use crate::shared::error::{db_err, AppResult};
#[cfg(test)]
use rusqlite::OptionalExtension;
use rusqlite::{params, TransactionBehavior};

const MAX_ROWS_PER_TRACE: usize = 101; // 100 named buckets + one overflow bucket.

/// Replace the bounded attribution snapshot for one trace in a single transaction.
///
/// The summary is intentionally stored as decimal text: the ledger uses saturating
/// arithmetic and can represent totals wider than SQLite's signed integer range.
pub(crate) fn replace_for_trace(
    database: &db::Db,
    trace_id: &str,
    providers: &[ProviderBucketSnapshot],
    updated_at: i64,
) -> AppResult<()> {
    let mut conn = database.open_connection()?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| db_err!("failed to start infinite retry usage transaction: {error}"))?;
    tx.execute(
        "DELETE FROM infinite_retry_provider_usage WHERE trace_id = ?1",
        params![trace_id],
    )
    .map_err(|error| db_err!("failed to clear infinite retry usage snapshot: {error}"))?;
    let mut statement = tx.prepare_cached(
        "INSERT INTO infinite_retry_provider_usage (
            trace_id, provider_key, provider_id, provider_name, attempt_count,
            usage_known_attempts, usage_unknown_attempts, input_tokens, output_tokens,
            total_tokens, reasoning_tokens, cache_read_input_tokens,
            cache_creation_input_tokens, cache_creation_5m_input_tokens,
            cache_creation_1h_input_tokens, cost_usd_femto, unpriced_attempts,
            complete, overflowed, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
    )
    .map_err(|error| db_err!("failed to prepare infinite retry usage snapshot: {error}"))?;

    for bucket in providers.iter().take(MAX_ROWS_PER_TRACE) {
        let provider_key = bucket
            .provider_id
            .map(|id| format!("id:{id}"))
            .unwrap_or_else(|| "other".to_string());
        let usage = &bucket.usage;
        statement
            .execute(params![
                trace_id,
                provider_key,
                bucket.provider_id,
                bucket.provider_name,
                bucket.attempt_count,
                usage.known_attempts,
                usage.unknown_attempts,
                usage.input_tokens,
                usage.output_tokens,
                usage.total_tokens,
                usage.reasoning_tokens,
                usage.cache_read_input_tokens,
                usage.cache_creation_input_tokens,
                usage.cache_creation_5m_input_tokens,
                usage.cache_creation_1h_input_tokens,
                usage.cost_usd_femto,
                usage.unpriced_attempts,
                if usage.complete { 1i64 } else { 0 },
                if usage.overflowed { 1i64 } else { 0 },
                updated_at,
            ])
            .map_err(|error| db_err!("failed to persist infinite retry usage bucket: {error}"))?;
    }
    drop(statement);
    tx.commit()
        .map_err(|error| db_err!("failed to commit infinite retry usage snapshot: {error}"))?;
    Ok(())
}

/// Return the number of persisted Provider buckets for a trace.
#[cfg(test)]
pub(crate) fn row_count(database: &db::Db, trace_id: &str) -> AppResult<i64> {
    let conn = database.open_connection()?;
    conn.query_row(
        "SELECT COUNT(*) FROM infinite_retry_provider_usage WHERE trace_id = ?1",
        params![trace_id],
        |row| row.get(0),
    )
    .map_err(|error| db_err!("failed to count infinite retry usage rows: {error}"))
}

/// Read one persisted cost value for a trace/provider key. This helper is kept
/// small and typed so quota/cost consumers can migrate to the child ledger
/// without parsing the activity JSON.
#[cfg(test)]
pub(crate) fn cost_for_key(
    database: &db::Db,
    trace_id: &str,
    provider_key: &str,
) -> AppResult<Option<String>> {
    let conn = database.open_connection()?;
    conn
        .query_row(
            "SELECT cost_usd_femto FROM infinite_retry_provider_usage WHERE trace_id = ?1 AND provider_key = ?2",
            params![trace_id, provider_key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| db_err!("failed to read infinite retry usage cost: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::infinite_retry::{InfiniteRetryLedger, UsageSample};

    #[test]
    fn replace_is_bounded_and_keeps_decimal_totals() {
        let directory = tempfile::tempdir().expect("temp database directory");
        let database = crate::db::init_for_tests(&directory.path().join("provider-usage.db"))
            .expect("initialize test database");
        let mut ledger = InfiniteRetryLedger::default();
        ledger.start_round(false);
        for id in 0..150_i64 {
            ledger.record_attempt(
                id,
                &format!("provider-{id}"),
                Some(200),
                "success",
                crate::gateway::infinite_retry::FailureCategory::Success,
                1,
                Some(UsageSample {
                    input_tokens: Some(1),
                    ..UsageSample::default()
                }),
                Some(u128::MAX),
            );
        }
        let snapshot = ledger.snapshot();
        replace_for_trace(&database, "trace-ledger", &snapshot.providers, 123).unwrap();
        assert_eq!(row_count(&database, "trace-ledger").unwrap(), 101);
        assert_eq!(
            cost_for_key(&database, "trace-ledger", "id:0").unwrap(),
            Some(u128::MAX.to_string())
        );
        assert_eq!(
            cost_for_key(&database, "trace-ledger", "other").unwrap(),
            Some(u128::MAX.saturating_mul(150).to_string())
        );
    }
}
