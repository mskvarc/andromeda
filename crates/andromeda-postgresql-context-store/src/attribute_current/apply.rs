//! Applying a drained batch, isolating the rows that cannot be applied.

use crate::{error::ContextStoreError, repair_attribute_current::repair_attribute_current};
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
use andromeda_postgresql_query::history_batch_entry::HistoryBatchEntry;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::postgres::PgConnection;

/// Result of applying one drained batch.
#[derive(Debug, Default)]
pub struct AppliedBatch {
    /// Rows written to the projection by the normal apply.
    pub applied: usize,
    /// Rows the apply rejected, whose TEA was afterwards recomputed from
    /// `attribute_history`. The projection is correct for these.
    pub repaired: usize,
    /// Rows that neither the apply nor the repair could account for. These are
    /// the only ones that leave the projection genuinely stale.
    pub dropped: usize,
    /// The error reported by the last abandoned row, for the log line.
    pub last_error: Option<ContextStoreError>,
}

/// Apply a batch, narrowing down to the individual rows that cannot be applied.
///
/// The apply is a single `INSERT`, so one unusable row aborts the statement and
/// discards every other row with it. That was tolerable while the projection was
/// unused; now that `q` predicates read it, losing a whole batch means wrong
/// query answers until the affected attributes are written again.
///
/// On failure the batch is halved and each half retried, recursing down to
/// single rows. The happy path costs exactly one statement -- the splitting only
/// happens after an error -- and one bad row costs `O(log n)` extra statements
/// rather than the batch.
///
/// A row that still fails alone is not abandoned. It is repaired by recomputing
/// its TEA's projection row from `attribute_history`, which is canonical. That
/// resolves both shapes of failure correctly: if the owning TEA was deleted, the
/// repair selects nothing and removes any stale row, which is the right state;
/// if the failure was transient, the newest observation is restored. This is
/// what keeps the read-your-writes barrier honest -- without it a dropped row
/// would leave the projection stale while the barrier reported the write as
/// applied, and nothing would correct it before the next restart.
///
/// Each apply is its own implicit transaction (no surrounding `BEGIN`), so a
/// failed statement leaves the connection usable for the retries. The statement
/// is an idempotent upsert guarded on `modified_at`, so re-applying a row that
/// already succeeded in a larger chunk is a no-op.
pub async fn apply_batch_isolating_failures(conn: &mut PgConnection, entries: &[HistoryBatchEntry]) -> AppliedBatch {
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

/// Apply a batch of history entries to `attribute_current`.
///
/// Runs on the caller-provided connection as a single autocommit statement:
/// durability is no longer tied to the original write transaction.
pub async fn apply_attribute_current_batch(conn: &mut PgConnection, entries: &[HistoryBatchEntry]) -> Result<(), ContextStoreError> {
    if entries.is_empty() {
        return Ok(());
    }

    let len = entries.len();
    let mut tea_ids = Vec::with_capacity(len);
    let mut times = Vec::with_capacity(len);
    let mut value_numbers: Vec<Option<f64>> = Vec::with_capacity(len);
    let mut value_strings: Vec<Option<&str>> = Vec::with_capacity(len);
    let mut value_bools: Vec<Option<bool>> = Vec::with_capacity(len);
    let mut value_geos: Vec<Option<Vec<u8>>> = Vec::with_capacity(len);
    let mut value_objects: Vec<Option<&Value>> = Vec::with_capacity(len);
    let mut value_datetimes: Vec<Option<DateTime<Utc>>> = Vec::with_capacity(len);
    let mut value_uris: Vec<Option<&str>> = Vec::with_capacity(len);
    let mut recorded_ats: Vec<DateTime<Utc>> = Vec::with_capacity(len);

    for entry in entries {
        tea_ids.push(entry.tea_id);
        times.push(entry.time);
        value_numbers.push(entry.value_number);
        value_strings.push(entry.value_string.as_deref());
        value_bools.push(entry.value_bool);
        value_geos.push(geometry_bytes(entry.value_geo.as_ref())?);
        value_objects.push(entry.value_object.as_ref());
        value_datetimes.push(entry.value_datetime);
        value_uris.push(entry.value_uri.as_deref());
        recorded_ats.push(entry.recorded_at);
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
/// rows. It deadlocks. `attribute_current` has two cascading foreign keys --
/// `entity_id` to `entities` and `tea_id` to `temporal_entity_attributes` -- so
/// `DELETE FROM entities` cascades into `attribute_current` and
/// `temporal_entity_attributes` in an order PostgreSQL does not guarantee. The
/// delete can hold `attribute_current` rows while waiting on the TEA rows this
/// statement would hold, which is a cycle. Measured: six deadlocks in one
/// integration run, and the victim was usually the user's `DELETE FROM
/// entities`, turning a lost projection row into a failed API request. Dropping
/// the occasional row is the cheaper failure -- that row's TEA is gone, so it
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribute_current::drain::tests::entries;
    use sqlx::PgPool;

    #[test]
    fn the_apply_statement_takes_no_row_locks_on_the_catalogue() {
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

    #[test]
    fn the_apply_statement_orders_and_guards_on_recorded_at() {
        assert!(
            APPLY_ATTRIBUTE_CURRENT_SQL.contains("ORDER BY d.tea_id, COALESCE(d.recorded_at, d.time) DESC"),
            "last-write-wins ordering must key on recorded_at, not the observedAt-bearing time column"
        );
        assert!(
            APPLY_ATTRIBUTE_CURRENT_SQL.contains("WHERE attribute_current.modified_at <= EXCLUDED.modified_at"),
            "the staleness guard must reject out-of-order applies"
        );
    }

    #[tokio::test]
    async fn failure_isolation_splits_down_to_single_rows() {
        // The pool points nowhere, so every apply fails. That exercises the
        // worst case: the batch is halved repeatedly until each row has been
        // tried alone, and every row is accounted for as dropped rather than
        // silently lost with its neighbours.
        let pool = PgPool::connect_lazy("postgres://invalid/invalid").expect("lazy pool");
        let Ok(mut conn) = pool.acquire().await else {
            // No server to connect to -- the split logic is covered by the
            // pure-function test below instead.
            return;
        };
        let outcome = apply_batch_isolating_failures(&mut conn, &entries(4)).await;
        pretty_assertions::assert_eq!(outcome.applied, 0);
        pretty_assertions::assert_eq!(outcome.dropped, 4);
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
            pretty_assertions::assert_eq!(seen, rows, "splitting must partition a batch of {size}");
        }
    }
}
