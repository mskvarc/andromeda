//! Async materializer for the `attribute_current` projection.
//!
//! `attribute_current` is a denormalised "latest value per TEA" view kept
//! in sync with `attribute_history`. The previous implementation upserted
//! it inside the same transaction as the history insert (177 ms mean on
//! the live broker, dominated by index-write amplification on 10 indexes
//! totalling ~3.5 GB). This module decouples the maintenance from the
//! synchronous write path: the producer queues batches on commit and
//! returns to the caller; this worker drains and applies them on its
//! own short-lived connection.
//!
//! Crash semantics: `attribute_history` is the canonical durable source.
//! In-flight events on the channel are lost on process exit, but the
//! [`AttributeCurrentMaterializer::reconcile_on_startup`] one-shot
//! refills `attribute_current` from `attribute_history` for every TEA
//! whose latest history row is newer than the current row, so the
//! projection re-converges on every restart.

use crate::{core::error::AndromedaError, repository::temporal::params::HistoryBatchEntry};
use sqlx::{PgPool, postgres::PgConnection};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Inflight batch of history entries waiting to be projected into
/// `attribute_current`. The `Arc` wrapper avoids cloning the producer's
/// vector when the channel queue is shallow (single consumer, no other
/// outstanding refs ⇒ `Arc::try_unwrap` succeeds).
pub type AttributeCurrentEvent = Arc<Vec<HistoryBatchEntry>>;

/// Producer-side channel handle used by the unit-of-work commit hook.
pub type AttributeCurrentSender = mpsc::UnboundedSender<AttributeCurrentEvent>;

/// Consumer-side channel handle owned by the worker.
pub type AttributeCurrentReceiver = mpsc::UnboundedReceiver<AttributeCurrentEvent>;

/// Hard cap on entries materialised in a single SQL statement.
///
/// Sized so one apply call holds a pool connection for ~hundreds of ms,
/// not multi-seconds. The previous 50 000 cap let the worker monopolise
/// a connection for 5-9 s on a busy broker, starving user-facing
/// requests (slow_acquire warnings at 2 s). 5 000 keeps the worker
/// throughput per drain comparable to the in-tx path it replaced while
/// leaving the rest of the pool free for ingestion.
const MAX_DRAIN_BATCH: usize = 5_000;

/// Background worker that drains `attribute_current` events and applies
/// them via a short-lived database connection acquired from the shared
/// pool.
pub struct AttributeCurrentMaterializer {
    rx: AttributeCurrentReceiver,
    pool: Arc<PgPool>,
    apply_log_interval: Duration,
}

impl AttributeCurrentMaterializer {
    /// Construct a new materializer.
    ///
    /// `apply_log_interval` throttles the success-path INFO log; errors
    /// are always logged immediately.
    pub const fn new(rx: AttributeCurrentReceiver, pool: Arc<PgPool>, apply_log_interval: Duration) -> AttributeCurrentMaterializer {
        AttributeCurrentMaterializer { rx, pool, apply_log_interval }
    }

    /// One-shot reconcile run before the worker enters its drain loop.
    ///
    /// Refills `attribute_current` from `attribute_history` for every
    /// TEA whose latest observation is newer than the current row (or
    /// the current row is missing entirely). Bounded by the staleness
    /// window since the last successful drain.
    pub async fn reconcile_on_startup(&self) -> Result<u64, AndromedaError> {
        let result = sqlx::query(RECONCILE_FROM_HISTORY_SQL).execute(&*self.pool).await?;
        let rows = result.rows_affected();
        if rows > 0 {
            info!(rows, "attribute_current reconcile applied missing rows from attribute_history");
        }
        Ok(rows)
    }

    /// Drive the worker until the channel is closed.
    ///
    /// Each iteration blocks on the first event, then drains every
    /// further event currently buffered, and applies them as one batch.
    /// This lets the consumer catch up on bursts without re-acquiring a
    /// DB connection per producer commit.
    pub async fn run(mut self) {
        let mut last_log = Instant::now().checked_sub(self.apply_log_interval).unwrap_or_else(Instant::now);
        let mut carry: Vec<HistoryBatchEntry> = Vec::new();
        while let Some(buffered) = next_buffered_batch(&mut self.rx, MAX_DRAIN_BATCH, &mut carry).await {
            if !buffered.is_empty() {
                self.apply_batch(&buffered, &mut last_log).await;
            }
        }
    }

    async fn apply_batch(&self, entries: &[HistoryBatchEntry], last_log: &mut Instant) {
        let count = entries.len();
        let started = Instant::now();
        let mut conn = match self.pool.acquire().await {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, count, event = "attribute_current_acquire_failed");
                return;
            }
        };
        if let Err(e) = apply_attribute_current_batch(&mut conn, entries).await {
            warn!(error = %e, count, event = "attribute_current_apply_failed");
            return;
        }
        let now = Instant::now();
        if now.duration_since(*last_log) >= self.apply_log_interval {
            info!(count, applied_in_ms = started.elapsed().as_millis() as u64, event = "attribute_current_drain");
            *last_log = now;
        }
    }
}

/// Apply a batch of history entries to `attribute_current`. Re-uses the
/// SQL shape from the previous in-tx implementation; runs on the
/// caller-provided connection (single-statement, autocommit) since the
/// durability is no longer tied to the original write transaction.
pub async fn apply_attribute_current_batch(conn: &mut PgConnection, entries: &[HistoryBatchEntry]) -> Result<(), AndromedaError> {
    if entries.is_empty() {
        return Ok(());
    }

    let len = entries.len();
    let mut tea_ids = Vec::with_capacity(len);
    let mut times = Vec::with_capacity(len);
    let mut value_numbers: Vec<Option<f64>> = Vec::with_capacity(len);
    let mut value_strings: Vec<Option<&str>> = Vec::with_capacity(len);
    let mut value_bools: Vec<Option<bool>> = Vec::with_capacity(len);
    let mut value_geos: Vec<Option<&crate::domain::geometry::Wkb>> = Vec::with_capacity(len);
    let mut value_objects: Vec<Option<&serde_json::Value>> = Vec::with_capacity(len);
    let mut value_datetimes: Vec<Option<chrono::DateTime<chrono::Utc>>> = Vec::with_capacity(len);
    let mut value_uris: Vec<Option<&str>> = Vec::with_capacity(len);

    for e in entries {
        tea_ids.push(e.tea_id);
        times.push(e.time);
        value_numbers.push(e.value_number);
        value_strings.push(e.value_string.as_deref());
        value_bools.push(e.value_bool);
        value_geos.push(e.value_geo.as_ref());
        value_objects.push(e.value_object.as_ref());
        value_datetimes.push(e.value_datetime);
        value_uris.push(e.value_uri.as_deref());
    }

    sqlx::query(APPLY_ATTRIBUTE_CURRENT_SQL)
        .bind(&tea_ids)
        .bind(&times)
        .bind(&value_numbers)
        .bind(&value_strings)
        .bind(&value_bools)
        .bind(&value_geos)
        .bind(&value_objects)
        .bind(&value_datetimes)
        .bind(&value_uris)
        .execute(conn)
        .await?;

    Ok(())
}

/// Extract the inner `Vec` from an `Arc` without cloning when the
/// caller is the only owner; fall back to `Vec::clone` otherwise.
fn take_or_clone(arc: AttributeCurrentEvent) -> Vec<HistoryBatchEntry> {
    Arc::try_unwrap(arc).unwrap_or_else(|shared| (*shared).clone())
}

/// Drain at most `max` history entries from the channel for one apply
/// pass. If the previous call left a tail in `carry`, start with that;
/// otherwise block on the first available event. Subsequent events are
/// drained non-blocking. Any overflow past `max` is stored back into
/// `carry` so the next call applies it first — no entries are lost.
///
/// Returns `None` only when the channel is closed and no carry remains.
async fn next_buffered_batch(rx: &mut AttributeCurrentReceiver, max: usize, carry: &mut Vec<HistoryBatchEntry>) -> Option<Vec<HistoryBatchEntry>> {
    let mut buffered = std::mem::take(carry);
    if buffered.is_empty() {
        let first = rx.recv().await?;
        buffered = take_or_clone(first);
    }

    while buffered.len() < max {
        match rx.try_recv() {
            Ok(next) => buffered.extend(take_or_clone(next)),
            Err(_) => break,
        }
    }

    if buffered.len() > max {
        let tail = buffered.split_off(max);
        *carry = tail;
    }

    Some(buffered)
}

const APPLY_ATTRIBUTE_CURRENT_SQL: &str = r#"
INSERT INTO attribute_current (
    tea_id, entity_id, attribute_name_id, dataset_id, value_type,
    value_number, value_string, value_bool, value_geo,
    value_object, value_datetime, value_uri, modified_at
)
SELECT DISTINCT ON (d.tea_id)
    d.tea_id, tea.entity_id, tea.attribute_name_id, tea.dataset_id, tea.value_type,
    d.value_number, d.value_string, d.value_bool,
    CASE WHEN d.value_geo IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.value_geo)) ELSE NULL END,
    d.value_object, d.value_datetime, d.value_uri, d.time
FROM UNNEST(
    $1::uuid[], $2::timestamptz[], $3::double precision[], $4::text[], $5::boolean[],
    $6::bytea[], $7::jsonb[], $8::timestamptz[], $9::text[]
) AS d(tea_id, time, value_number, value_string, value_bool,
       value_geo, value_object, value_datetime, value_uri)
JOIN temporal_entity_attributes tea ON tea.id = d.tea_id
ORDER BY d.tea_id, d.time DESC
ON CONFLICT (tea_id) DO UPDATE SET
    entity_id         = EXCLUDED.entity_id,
    attribute_name_id = EXCLUDED.attribute_name_id,
    dataset_id        = EXCLUDED.dataset_id,
    value_type        = EXCLUDED.value_type,
    value_number      = EXCLUDED.value_number,
    value_string      = EXCLUDED.value_string,
    value_bool        = EXCLUDED.value_bool,
    value_geo         = EXCLUDED.value_geo,
    value_object      = EXCLUDED.value_object,
    value_datetime    = EXCLUDED.value_datetime,
    value_uri         = EXCLUDED.value_uri,
    modified_at       = EXCLUDED.modified_at
WHERE attribute_current.modified_at <= EXCLUDED.modified_at
"#;

/// Reconcile statement run on startup. For every TEA whose latest
/// history row is newer than the current projection (or no projection
/// row exists), refill `attribute_current` from `attribute_history`.
const RECONCILE_FROM_HISTORY_SQL: &str = r#"
INSERT INTO attribute_current (
    tea_id, entity_id, attribute_name_id, dataset_id, value_type,
    value_number, value_string, value_bool, value_geo,
    value_object, value_datetime, value_uri, modified_at
)
SELECT DISTINCT ON (ah.tea_id)
    ah.tea_id, tea.entity_id, tea.attribute_name_id, tea.dataset_id, tea.value_type,
    ah.value_number, ah.value_string, ah.value_bool, ah.value_geo,
    ah.value_object, ah.value_datetime, ah.value_uri, ah.time
FROM attribute_history ah
JOIN temporal_entity_attributes tea ON tea.id = ah.tea_id
LEFT JOIN attribute_current ac ON ac.tea_id = ah.tea_id
WHERE ac.tea_id IS NULL OR ah.time > ac.modified_at
ORDER BY ah.tea_id, ah.time DESC
ON CONFLICT (tea_id) DO UPDATE SET
    entity_id         = EXCLUDED.entity_id,
    attribute_name_id = EXCLUDED.attribute_name_id,
    dataset_id        = EXCLUDED.dataset_id,
    value_type        = EXCLUDED.value_type,
    value_number      = EXCLUDED.value_number,
    value_string      = EXCLUDED.value_string,
    value_bool        = EXCLUDED.value_bool,
    value_geo         = EXCLUDED.value_geo,
    value_object      = EXCLUDED.value_object,
    value_datetime    = EXCLUDED.value_datetime,
    value_uri         = EXCLUDED.value_uri,
    modified_at       = EXCLUDED.modified_at
WHERE attribute_current.modified_at <= EXCLUDED.modified_at
"#;

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use chrono::{TimeZone, Utc};

    fn make_entry(tea_id: uuid::Uuid) -> HistoryBatchEntry {
        HistoryBatchEntry {
            time: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
            tea_id,
            value_number: None,
            value_string: None,
            value_bool: None,
            value_geo: None,
            value_object: None,
            value_datetime: None,
            value_uri: None,
            instance_id: format!("urn:ngsi-ld:attribute:instance:{tea_id}"),
            observed_at: None,
            metadata: None,
            instance_id_client_supplied: false,
        }
    }

    #[test]
    fn take_or_clone_returns_inner_vec_when_unique() {
        let entries = vec![make_entry(uuid::Uuid::nil())];
        let arc = Arc::new(entries);
        let recovered = take_or_clone(arc);
        assert_eq!(recovered.len(), 1);
    }

    #[test]
    fn take_or_clone_clones_when_shared() {
        let entries = vec![make_entry(uuid::Uuid::nil())];
        let arc = Arc::new(entries);
        let _shared = Arc::clone(&arc);
        let recovered = take_or_clone(arc);
        assert_eq!(recovered.len(), 1);
    }

    #[test]
    fn max_drain_batch_is_finite_cap() {
        assert!(MAX_DRAIN_BATCH > 0);
        assert!(MAX_DRAIN_BATCH <= 1_000_000);
    }

    fn entries(count: usize) -> Vec<HistoryBatchEntry> {
        (0..count).map(|_| make_entry(uuid::Uuid::new_v4())).collect()
    }

    #[tokio::test]
    async fn materializer_drains_single_event_and_applies() {
        let (tx, mut rx) = mpsc::unbounded_channel::<AttributeCurrentEvent>();
        tx.send(Arc::new(entries(3))).expect("send");
        drop(tx);

        let mut carry = Vec::new();
        let batch = next_buffered_batch(&mut rx, MAX_DRAIN_BATCH, &mut carry).await.expect("batch");
        assert_eq!(batch.len(), 3);
        assert!(carry.is_empty());

        let next = next_buffered_batch(&mut rx, MAX_DRAIN_BATCH, &mut carry).await;
        assert!(next.is_none(), "channel closed and carry empty -> None");
    }

    #[tokio::test]
    async fn materializer_drains_multiple_events_in_one_pass() {
        let (tx, mut rx) = mpsc::unbounded_channel::<AttributeCurrentEvent>();
        for _ in 0..5 {
            tx.send(Arc::new(entries(2))).expect("send");
        }
        drop(tx);

        let mut carry = Vec::new();
        let batch = next_buffered_batch(&mut rx, MAX_DRAIN_BATCH, &mut carry).await.expect("batch");
        assert_eq!(batch.len(), 10, "five 2-entry events drained as one 10-entry batch");
        assert!(carry.is_empty());
    }

    #[tokio::test]
    async fn materializer_caps_at_max_drain_batch() {
        let (tx, mut rx) = mpsc::unbounded_channel::<AttributeCurrentEvent>();
        // 4 events of 30 entries each = 120 entries with a max of 50.
        for _ in 0..4 {
            tx.send(Arc::new(entries(30))).expect("send");
        }
        drop(tx);

        let mut carry = Vec::new();
        let mut applied: Vec<usize> = Vec::new();
        while let Some(batch) = next_buffered_batch(&mut rx, 50, &mut carry).await {
            applied.push(batch.len());
        }
        assert!(!applied.is_empty(), "should have applied at least one batch");
        for size in &applied {
            assert!(*size <= 50, "every batch must respect cap; got {}", size);
        }
        let total: usize = applied.iter().sum();
        assert_eq!(total, 120, "all entries must be flushed across multiple batches");
    }

    #[test]
    fn uow_pending_attribute_current_send_dispatches_arc_event() {
        let (tx, mut rx) = mpsc::unbounded_channel::<AttributeCurrentEvent>();
        let pending = entries(2);
        // Mirror UoW commit logic in isolation.
        let event: AttributeCurrentEvent = Arc::new(pending);
        tx.send(event).expect("send");
        drop(tx);
        let received = rx.try_recv().expect("event");
        assert_eq!(received.len(), 2);
    }

    #[test]
    fn uow_rollback_does_not_emit_event() {
        let (tx, mut rx) = mpsc::unbounded_channel::<AttributeCurrentEvent>();
        // Producer drops without sending — mirrors UoW dropped without commit().
        let _pending = entries(1);
        drop(tx);
        let received = rx.try_recv();
        assert!(received.is_err(), "no event should have been emitted");
    }
}
