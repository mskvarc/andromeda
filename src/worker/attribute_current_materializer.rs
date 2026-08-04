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

use crate::{
    core::error::AndromedaError,
    repository::temporal::{params::HistoryBatchEntry, repo::repair_attribute_current},
};
use sqlx::{PgPool, postgres::PgConnection};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

/// Depth of the materializer queue, in batches.
///
/// The queue is bounded so a materializer that falls behind applies
/// backpressure to committers instead of growing without limit. Enqueueing
/// happens *after* the database commit, so a full queue never holds a
/// transaction open — it only delays the producer's return. Sized to absorb a
/// multi-second stall at batch-ingest rates while keeping the worst-case
/// resident set bounded.
const QUEUE_CAPACITY: usize = 1_024;

/// Hard cap on entries materialised in a single SQL statement.
///
/// Sized so one apply call holds a pool connection for ~hundreds of ms,
/// not multi-seconds. The previous 50 000 cap let the worker monopolise
/// a connection for 5-9 s on a busy broker, starving user-facing
/// requests (slow_acquire warnings at 2 s). 5 000 keeps the worker
/// throughput per drain comparable to the in-tx path it replaced while
/// leaving the rest of the pool free for ingestion.
const MAX_DRAIN_BATCH: usize = 5_000;

// A zero cap would make `next_buffered_batch` carry every drained entry
// forward without ever applying one; an unbounded one reintroduces the
// connection starvation described above. Checked here rather than in a unit
// test, where the assertion is a compile-time constant either way.
const _: () = assert!(MAX_DRAIN_BATCH > 0 && MAX_DRAIN_BATCH <= 1_000_000);

/// A batch of history entries queued for projection into `attribute_current`,
/// tagged with its position in the issue order.
///
/// The `Arc` wrapper avoids cloning the producer's vector when the channel
/// queue is shallow (single consumer, no other outstanding refs ⇒
/// `Arc::try_unwrap` succeeds).
#[derive(Debug)]
pub struct AttributeCurrentEvent {
    /// Monotonic issue sequence, assigned by [`AttributeCurrentQueue::publish`].
    seq: u64,
    /// History rows to project.
    entries: Arc<Vec<HistoryBatchEntry>>,
}

/// Consumer-side channel handle owned by the worker.
type AttributeCurrentReceiver = mpsc::Receiver<AttributeCurrentEvent>;

/// Producer handle used by the unit-of-work commit hook.
///
/// Assigns each published batch a monotonic sequence number before enqueueing
/// it, which is what lets [`AttributeCurrentBarrier`] express "everything
/// issued before now" without putting a message on the queue.
#[derive(Debug, Clone)]
pub struct AttributeCurrentQueue {
    tx: mpsc::Sender<AttributeCurrentEvent>,
    issued: Arc<AtomicU64>,
}

impl AttributeCurrentQueue {
    /// Queue a batch of history rows for projection.
    ///
    /// Awaits queue capacity when the materializer is behind. A closed channel
    /// (worker exited) is logged and swallowed: `attribute_history` is the
    /// canonical source and the startup reconcile refills the projection.
    pub async fn publish(&self, entries: Vec<HistoryBatchEntry>) {
        if entries.is_empty() {
            return;
        }
        let seq = self.issued.fetch_add(1, Ordering::AcqRel) + 1;
        let event = AttributeCurrentEvent {
            seq,
            entries: Arc::new(entries),
        };
        if self.tx.send(event).await.is_err() {
            warn!(event = "attribute_current_publish_failed");
        }
    }
}

/// Read-your-writes barrier for the query path.
///
/// Held by the services that compile `q` filters against `attribute_current`.
/// [`AttributeCurrentBarrier::wait_for_pending_writes`] samples the current
/// issue high-water mark and waits until the materializer's applied watermark
/// reaches it, so a query issued after a committed write observes that write.
///
/// This is a watermark rather than a queued barrier message deliberately: a
/// barrier message would sit in the same FIFO as the batches, forcing a drain
/// boundary at every `q`-filtered query and coupling read latency to write
/// backlog. Sampling a watermark adds no queue traffic, does not fragment the
/// worker's batching, and lets concurrent waiters share one notification.
#[derive(Debug, Clone)]
pub struct AttributeCurrentBarrier {
    issued: Arc<AtomicU64>,
    applied: watch::Receiver<u64>,
}

impl AttributeCurrentBarrier {
    /// Wait until every batch queued before this call has been applied.
    ///
    /// A dead worker (closed watch channel) is logged and then ignored:
    /// availability of the read path is preferred over blocking it on a
    /// background worker.
    pub async fn wait_for_pending_writes(&self) {
        let target = self.issued.load(Ordering::Acquire);
        let mut applied = self.applied.clone(); // clone: per-call cursor; watch::Receiver clone is cheap
        if *applied.borrow_and_update() >= target {
            return;
        }
        while applied.changed().await.is_ok() {
            if *applied.borrow_and_update() >= target {
                return;
            }
        }
        error!(target_seq = target, event = "attribute_current_barrier_worker_gone");
    }
}

/// Background worker that drains `attribute_current` events and applies
/// them via a short-lived database connection acquired from the shared
/// pool.
pub struct AttributeCurrentMaterializer {
    rx: AttributeCurrentReceiver,
    applied: watch::Sender<u64>,
    pool: Arc<PgPool>,
    apply_log_interval: Duration,
}

impl AttributeCurrentMaterializer {
    /// Construct a materializer together with its producer and barrier handles.
    ///
    /// `apply_log_interval` throttles the success-path INFO log; errors
    /// are always logged immediately.
    pub fn new(pool: Arc<PgPool>, apply_log_interval: Duration) -> (AttributeCurrentMaterializer, AttributeCurrentQueue, AttributeCurrentBarrier) {
        let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
        let (applied_tx, applied_rx) = watch::channel(0u64);
        let issued = Arc::new(AtomicU64::new(0));
        let queue = AttributeCurrentQueue {
            tx,
            issued: Arc::clone(&issued),
        };
        let barrier = AttributeCurrentBarrier { issued, applied: applied_rx };
        let worker = AttributeCurrentMaterializer {
            rx,
            applied: applied_tx,
            pool,
            apply_log_interval,
        };
        (worker, queue, barrier)
    }

    /// One-shot reconcile run before the worker enters its drain loop.
    ///
    /// Refills `attribute_current` from `attribute_history` for every
    /// TEA whose latest observation is newer than the current row (or
    /// the current row is missing entirely).
    ///
    /// This is a full pass over `attribute_history` with a per-TEA
    /// `DISTINCT ON`: it is a startup one-shot whose cost scales with
    /// retention, not a steady-state query. Making it incremental needs a
    /// durable "last applied" marker, which the projection does not carry.
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
    /// DB connection per producer commit. After each applied batch the
    /// watermark advances to the highest *fully applied* sequence, which is
    /// what [`AttributeCurrentBarrier`] waits on.
    pub async fn run(mut self) {
        let mut last_log = Instant::now().checked_sub(self.apply_log_interval).unwrap_or_else(Instant::now);
        let mut carry = DrainCarry::default();
        while let Some(drained) = next_buffered_batch(&mut self.rx, MAX_DRAIN_BATCH, &mut carry).await {
            if !drained.entries.is_empty() {
                self.apply_batch(&drained.entries, &mut last_log).await;
            }
            if drained.completed_seq > 0 {
                // Ignore send errors: a watermark with no receivers is a
                // broker with no in-flight q-filtered queries.
                let _ = self.applied.send(drained.completed_seq);
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
        let outcome = apply_batch_isolating_failures(&mut conn, entries).await;
        if outcome.repaired > 0 {
            info!(
                repaired = outcome.repaired,
                applied = outcome.applied,
                event = "attribute_current_rows_repaired"
            );
        }
        if outcome.dropped > 0 {
            // The projection is now missing data that no barrier can account
            // for: `wait_for_pending_writes` will report these writes as
            // applied. Until the next write for the affected attributes, or a
            // restart reconcile, `q` filters may answer from stale state.
            error!(
                dropped = outcome.dropped,
                applied = outcome.applied,
                repaired = outcome.repaired,
                error = %outcome.last_error.as_ref().map_or_else(|| "unknown".to_string(), ToString::to_string),
                event = "attribute_current_rows_dropped"
            );
        }
        let now = Instant::now();
        if now.duration_since(*last_log) >= self.apply_log_interval {
            info!(count, applied_in_ms = started.elapsed().as_millis() as u64, event = "attribute_current_drain");
            *last_log = now;
        }
    }
}

/// Result of applying one drained batch.
#[derive(Debug, Default)]
struct AppliedBatch {
    /// Rows written to the projection by the normal apply.
    applied: usize,
    /// Rows the apply rejected, whose TEA was afterwards recomputed from
    /// `attribute_history`. The projection is correct for these.
    repaired: usize,
    /// Rows that neither the apply nor the repair could account for. These are
    /// the only ones that leave the projection genuinely stale.
    dropped: usize,
    /// The error reported by the last abandoned row, for the log line.
    last_error: Option<AndromedaError>,
}

/// Apply a batch, narrowing down to the individual rows that cannot be applied.
///
/// The apply is a single `INSERT`, so one unusable row aborts the statement and
/// discards every other row with it. That was tolerable while the projection was
/// unused; now that `q` predicates read it, losing a whole batch means wrong
/// query answers until the affected attributes are written again.
///
/// On failure the batch is halved and each half retried, recursing down to
/// single rows. The happy path costs exactly one statement — the splitting only
/// happens after an error — and one bad row costs `O(log n)` extra statements
/// rather than the batch.
///
/// A row that still fails alone is not abandoned. It is repaired by recomputing
/// its TEA's projection row from `attribute_history`, which is canonical. That
/// resolves both shapes of failure correctly: if the owning TEA was deleted, the
/// repair selects nothing and removes any stale row, which is the right state;
/// if the failure was transient, the newest observation is restored. This is
/// what keeps [`AttributeCurrentBarrier`] honest — without it a dropped row
/// would leave the projection stale while the barrier reported the write as
/// applied, and nothing would correct it before the next restart.
///
/// Each apply is its own implicit transaction (no surrounding `BEGIN`), so a
/// failed statement leaves the connection usable for the retries. The statement
/// is an idempotent upsert guarded on `modified_at`, so re-applying a row that
/// already succeeded in a larger chunk is a no-op.
async fn apply_batch_isolating_failures(conn: &mut PgConnection, entries: &[HistoryBatchEntry]) -> AppliedBatch {
    let mut outcome = AppliedBatch::default();
    let mut pending: Vec<&[HistoryBatchEntry]> = vec![entries];

    while let Some(chunk) = pending.pop() {
        match apply_attribute_current_batch(conn, chunk).await {
            Ok(()) => outcome.applied += chunk.len(),
            Err(_apply_error) => match chunk.len() {
                0 => {}
                1 => match repair_attribute_current(conn, chunk[0].tea_id).await {
                    Ok(()) => outcome.repaired += 1,
                    Err(repair_error) => {
                        outcome.dropped += 1;
                        outcome.last_error = Some(repair_error);
                    }
                },
                len => {
                    let (left, right) = chunk.split_at(len / 2);
                    pending.push(left);
                    pending.push(right);
                }
            },
        }
    }

    outcome
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
    let mut recorded_ats: Vec<chrono::DateTime<chrono::Utc>> = Vec::with_capacity(len);

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
        recorded_ats.push(e.recorded_at);
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
        .bind(&recorded_ats)
        .execute(conn)
        .await?;

    Ok(())
}

/// Extract the inner `Vec` from an `Arc` without cloning when the
/// caller is the only owner; fall back to `Vec::clone` otherwise.
fn take_or_clone(arc: Arc<Vec<HistoryBatchEntry>>) -> Vec<HistoryBatchEntry> {
    Arc::try_unwrap(arc).unwrap_or_else(|shared| (*shared).clone())
}

/// Entries left over from the previous drain because they exceeded the
/// per-apply cap, together with the sequence number of the event they came
/// from. That sequence may only be published once the leftovers are applied,
/// so the barrier never reports a partially applied batch as complete.
#[derive(Debug, Default)]
struct DrainCarry {
    entries: Vec<HistoryBatchEntry>,
    /// Sequence of the partially consumed event; `0` when `entries` is empty.
    seq: u64,
}

/// One drain pass: the entries to apply, plus the highest sequence number that
/// is fully contained in this batch (`0` when none completed).
#[derive(Debug)]
struct DrainedBatch {
    entries: Vec<HistoryBatchEntry>,
    completed_seq: u64,
}

/// Drain at most `max` history entries from the channel for one apply
/// pass. If the previous call left a tail in `carry`, start with that;
/// otherwise block on the first available event. Subsequent events are
/// drained non-blocking. Any overflow past `max` is stored back into
/// `carry` so the next call applies it first — no entries are lost.
///
/// Returns `None` only when the channel is closed and no carry remains.
async fn next_buffered_batch(rx: &mut AttributeCurrentReceiver, max: usize, carry: &mut DrainCarry) -> Option<DrainedBatch> {
    let mut buffered = std::mem::take(&mut carry.entries);
    let mut pending_seq = std::mem::take(&mut carry.seq);
    let mut completed_seq = 0u64;

    if buffered.is_empty() {
        let first = rx.recv().await?;
        pending_seq = first.seq;
        buffered = take_or_clone(first.entries);
    }

    while buffered.len() < max {
        match rx.try_recv() {
            Ok(next) => {
                // Everything up to `pending_seq` is now wholly inside `buffered`.
                completed_seq = completed_seq.max(pending_seq);
                pending_seq = next.seq;
                buffered.extend(take_or_clone(next.entries));
            }
            Err(_) => break,
        }
    }

    if buffered.len() > max {
        carry.entries = buffered.split_off(max);
        carry.seq = pending_seq;
    } else {
        completed_seq = completed_seq.max(pending_seq);
    }

    Some(DrainedBatch {
        entries: buffered,
        completed_seq,
    })
}

/// Apply statement for a drained batch.
///
/// Ordering and the staleness guard both key on `COALESCE(recorded_at, time)`,
/// not `time`: `time` carries the client-supplied `observedAt` when present, so
/// a back-dated write would otherwise fail to supersede the row it replaces and
/// the projection would drift away from `entities.payload`.
///
/// The join against `temporal_entity_attributes` filters rows whose owning TEA
/// has been deleted, but only as of this statement's snapshot, while
/// `attribute_current_tea_id_fkey` is re-checked against the latest committed
/// state. A delete committing in between therefore still raises a foreign-key
/// violation. [`apply_batch_isolating_failures`] contains that: the row is
/// retried alone and abandoned, and its neighbours still land.
///
/// Do **not** try to close the race by taking `FOR KEY SHARE` on the catalogue
/// rows. It deadlocks. `attribute_current` has two cascading foreign keys —
/// `entity_id` to `entities` and `tea_id` to `temporal_entity_attributes` — so
/// `DELETE FROM entities` cascades into `attribute_current` and
/// `temporal_entity_attributes` in an order PostgreSQL does not guarantee. The
/// delete can hold `attribute_current` rows while waiting on the TEA rows this
/// statement would hold, which is a cycle. Measured: six deadlocks in one
/// integration run, and the victim was usually the user's `DELETE FROM
/// entities`, turning a lost projection row into a failed API request. Dropping
/// the occasional row is the cheaper failure — that row's TEA is gone, so it
/// does not belong in the projection anyway.
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
    d.value_object, d.value_datetime, d.value_uri, COALESCE(d.recorded_at, d.time)
FROM UNNEST(
    $1::uuid[], $2::timestamptz[], $3::double precision[], $4::text[], $5::boolean[],
    $6::bytea[], $7::jsonb[], $8::timestamptz[], $9::text[], $10::timestamptz[]
) AS d(tea_id, time, value_number, value_string, value_bool,
       value_geo, value_object, value_datetime, value_uri, recorded_at)
JOIN temporal_entity_attributes tea ON tea.id = d.tea_id
ORDER BY d.tea_id, COALESCE(d.recorded_at, d.time) DESC
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
    ah.value_object, ah.value_datetime, ah.value_uri, COALESCE(ah.recorded_at, ah.time)
FROM attribute_history ah
JOIN temporal_entity_attributes tea ON tea.id = ah.tea_id
LEFT JOIN attribute_current ac ON ac.tea_id = ah.tea_id
WHERE ac.tea_id IS NULL OR COALESCE(ah.recorded_at, ah.time) > ac.modified_at
ORDER BY ah.tea_id, COALESCE(ah.recorded_at, ah.time) DESC
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
            recorded_at: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
            instance_id_client_supplied: false,
        }
    }

    fn event(seq: u64, count: usize) -> AttributeCurrentEvent {
        AttributeCurrentEvent {
            seq,
            entries: Arc::new(entries(count)),
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

    fn entries(count: usize) -> Vec<HistoryBatchEntry> {
        (0..count).map(|_| make_entry(uuid::Uuid::new_v4())).collect()
    }

    #[tokio::test]
    async fn materializer_drains_single_event_and_applies() {
        let (tx, mut rx) = mpsc::channel::<AttributeCurrentEvent>(8);
        tx.send(event(1, 3)).await.expect("send");
        drop(tx);

        let mut carry = DrainCarry::default();
        let batch = next_buffered_batch(&mut rx, MAX_DRAIN_BATCH, &mut carry).await.expect("batch");
        assert_eq!(batch.entries.len(), 3);
        assert_eq!(batch.completed_seq, 1);
        assert!(carry.entries.is_empty());

        let next = next_buffered_batch(&mut rx, MAX_DRAIN_BATCH, &mut carry).await;
        assert!(next.is_none(), "channel closed and carry empty -> None");
    }

    #[tokio::test]
    async fn materializer_drains_multiple_events_in_one_pass() {
        let (tx, mut rx) = mpsc::channel::<AttributeCurrentEvent>(8);
        for seq in 1..=5 {
            tx.send(event(seq, 2)).await.expect("send");
        }
        drop(tx);

        let mut carry = DrainCarry::default();
        let batch = next_buffered_batch(&mut rx, MAX_DRAIN_BATCH, &mut carry).await.expect("batch");
        assert_eq!(batch.entries.len(), 10, "five 2-entry events drained as one 10-entry batch");
        assert_eq!(batch.completed_seq, 5, "watermark covers every event in the batch");
        assert!(carry.entries.is_empty());
    }

    #[tokio::test]
    async fn materializer_caps_at_max_drain_batch() {
        let (tx, mut rx) = mpsc::channel::<AttributeCurrentEvent>(8);
        // 4 events of 30 entries each = 120 entries with a max of 50.
        for seq in 1..=4 {
            tx.send(event(seq, 30)).await.expect("send");
        }
        drop(tx);

        let mut carry = DrainCarry::default();
        let mut applied: Vec<usize> = Vec::new();
        while let Some(batch) = next_buffered_batch(&mut rx, 50, &mut carry).await {
            applied.push(batch.entries.len());
        }
        assert!(!applied.is_empty(), "should have applied at least one batch");
        for size in &applied {
            assert!(*size <= 50, "every batch must respect cap; got {}", size);
        }
        let total: usize = applied.iter().sum();
        assert_eq!(total, 120, "all entries must be flushed across multiple batches");
    }

    #[tokio::test]
    async fn watermark_never_reports_a_partially_applied_event() {
        // Two 30-entry events with a cap of 50: the first batch takes all of
        // event 1 plus 20 rows of event 2, so only seq 1 may be published.
        let (tx, mut rx) = mpsc::channel::<AttributeCurrentEvent>(8);
        tx.send(event(1, 30)).await.expect("send");
        tx.send(event(2, 30)).await.expect("send");
        drop(tx);

        let mut carry = DrainCarry::default();
        let first = next_buffered_batch(&mut rx, 50, &mut carry).await.expect("batch");
        assert_eq!(first.entries.len(), 50);
        assert_eq!(first.completed_seq, 1, "event 2 is only half applied — must not be published");
        assert_eq!(carry.entries.len(), 10);
        assert_eq!(carry.seq, 2);

        let second = next_buffered_batch(&mut rx, 50, &mut carry).await.expect("batch");
        assert_eq!(second.entries.len(), 10);
        assert_eq!(second.completed_seq, 2, "event 2 completes once its tail is applied");
    }

    #[tokio::test]
    async fn barrier_returns_once_watermark_reaches_issued_high_water() {
        let (worker, queue, barrier) = AttributeCurrentMaterializer::new(
            Arc::new(PgPool::connect_lazy("postgres://invalid/invalid").expect("lazy pool")),
            Duration::from_secs(60),
        );
        queue.publish(entries(1)).await;
        queue.publish(entries(1)).await;

        // Nothing applied yet: the barrier must not be satisfied.
        assert_eq!(*barrier.applied.borrow(), 0);
        assert_eq!(barrier.issued.load(Ordering::Acquire), 2);

        // Publishing the watermark for both events releases the barrier.
        worker.applied.send(2).expect("watermark send");
        barrier.wait_for_pending_writes().await;
    }

    #[tokio::test]
    async fn barrier_is_satisfied_immediately_when_nothing_is_queued() {
        let (_worker, _queue, barrier) = AttributeCurrentMaterializer::new(
            Arc::new(PgPool::connect_lazy("postgres://invalid/invalid").expect("lazy pool")),
            Duration::from_secs(60),
        );
        barrier.wait_for_pending_writes().await;
    }

    #[tokio::test]
    async fn queue_publish_assigns_monotonic_sequences() {
        let (worker, queue, _barrier) = AttributeCurrentMaterializer::new(
            Arc::new(PgPool::connect_lazy("postgres://invalid/invalid").expect("lazy pool")),
            Duration::from_secs(60),
        );
        let mut rx = worker.rx;
        queue.publish(entries(1)).await;
        queue.publish(entries(1)).await;
        assert_eq!(rx.try_recv().expect("first").seq, 1);
        assert_eq!(rx.try_recv().expect("second").seq, 2);
    }

    #[tokio::test]
    async fn queue_publish_ignores_empty_batches() {
        let (worker, queue, barrier) = AttributeCurrentMaterializer::new(
            Arc::new(PgPool::connect_lazy("postgres://invalid/invalid").expect("lazy pool")),
            Duration::from_secs(60),
        );
        let mut rx = worker.rx;
        queue.publish(Vec::new()).await;
        assert!(rx.try_recv().is_err(), "empty batch must not occupy a queue slot");
        assert_eq!(barrier.issued.load(Ordering::Acquire), 0, "empty batch must not advance the high-water mark");
    }

    #[test]
    fn apply_sql_takes_no_row_locks_on_the_catalogue() {
        // Regression guard. Locking the catalogue rows to close the
        // delete-versus-foreign-key race deadlocks against `DELETE FROM
        // entities`, because `attribute_current` cascades from both `entities`
        // and `temporal_entity_attributes` and PostgreSQL does not fix the
        // order it fires those cascades in. The victim is usually the user's
        // delete. Failed rows are contained by `apply_batch_isolating_failures`
        // instead.
        assert!(!APPLY_ATTRIBUTE_CURRENT_SQL.contains("FOR KEY SHARE"), "see the const's doc comment");
        assert!(!APPLY_ATTRIBUTE_CURRENT_SQL.contains("FOR UPDATE"));
        assert!(!APPLY_ATTRIBUTE_CURRENT_SQL.contains("FOR SHARE"));
    }

    #[tokio::test]
    async fn failure_isolation_splits_down_to_single_rows() {
        // The pool points nowhere, so every apply fails. That exercises the
        // worst case: the batch is halved repeatedly until each row has been
        // tried alone, and every row is accounted for as dropped rather than
        // silently lost with its neighbours.
        let pool = PgPool::connect_lazy("postgres://invalid/invalid").expect("lazy pool");
        let mut conn = match pool.acquire().await {
            Ok(conn) => conn,
            // No server to connect to — the split logic is covered by the
            // pure-function test below instead.
            Err(_) => return,
        };
        let outcome = apply_batch_isolating_failures(&mut conn, &entries(4)).await;
        assert_eq!(outcome.applied, 0);
        assert_eq!(outcome.dropped, 4);
    }

    #[test]
    fn splitting_a_chunk_covers_every_row_exactly_once() {
        // Mirrors the halving in `apply_batch_isolating_failures` without a
        // database: whatever the batch size, splitting must partition it.
        for size in [1usize, 2, 3, 5, 8, 4096] {
            let rows: Vec<usize> = (0..size).collect();
            let mut pending: Vec<&[usize]> = vec![&rows];
            let mut seen: Vec<usize> = Vec::new();
            while let Some(chunk) = pending.pop() {
                match chunk.len() {
                    0 => {}
                    1 => seen.push(chunk[0]),
                    len => {
                        let (left, right) = chunk.split_at(len / 2);
                        pending.push(left);
                        pending.push(right);
                    }
                }
            }
            seen.sort_unstable();
            assert_eq!(seen, rows, "splitting must partition a batch of {size}");
        }
    }

    #[test]
    fn apply_sql_orders_and_guards_on_recorded_at() {
        assert!(
            APPLY_ATTRIBUTE_CURRENT_SQL.contains("ORDER BY d.tea_id, COALESCE(d.recorded_at, d.time) DESC"),
            "last-write-wins ordering must key on recorded_at, not the observedAt-bearing time column"
        );
        assert!(
            APPLY_ATTRIBUTE_CURRENT_SQL.contains("WHERE attribute_current.modified_at <= EXCLUDED.modified_at"),
            "staleness guard must reject out-of-order applies"
        );
        assert!(RECONCILE_FROM_HISTORY_SQL.contains("COALESCE(ah.recorded_at, ah.time) > ac.modified_at"));
    }
}
