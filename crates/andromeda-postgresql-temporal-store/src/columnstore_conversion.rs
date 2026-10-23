//! Converting aged history chunks to the columnstore, by the broker itself.
//!
//! A `TimescaleDB` columnstore policy runs as a background job inside each
//! database, and each database's scheduler and each running job take a worker
//! from `timescaledb.max_background_workers`. A server holding more tenant
//! databases than that leaves some without a scheduler, and their history then
//! silently stays in the rowstore forever. The broker therefore converts chunks
//! itself, on a periodic sweep beside retention: no tenant's history depends on
//! a background worker, and every conversion is visible in the broker's own
//! outcome.
//!
//! A chunk qualifies once its whole range ends at or before the horizon
//! (`temporal.columnstore_after` before now), it is still in the rowstore and
//! the sweep has not kept it there ([`crate::rowstore_history_chunks`]); a
//! converted or kept chunk is never selected again. Each chunk is handled in a
//! transaction of its own, so a lock is held only for one chunk, under these
//! transaction-scoped settings:
//!
//! - an advisory lock keyed on the chunk, taken without waiting: replicas of
//!   one broker sweep the same tenant, and the one that holds the lock converts
//!   the chunk while the others pass over it; the chunk's state is read again
//!   under the lock, since another replica may have converted or kept it in
//!   between;
//! - no `statement_timeout`: no request waits on the sweep;
//! - a bounded `lock_timeout`: a conversion queued behind a long transaction
//!   would hold up every write that reaches the chunk after it.
//!
//! Under the lock the chunk's rows and segments are counted first. A chunk
//! whose rows per segment fall below the layout's break-even
//! ([`HypertableName::segment_break_even`]) would convert to more bytes than
//! its rowstore heap and indexes take, so it is recorded as kept and left as
//! it is.
//!
//! A conversion is first tried with
//! `timescaledb.compress_truncate_behaviour = 'truncate_only'`, under a
//! savepoint: it truncates the chunk's rowstore relation and indexes, and
//! `TimescaleDB` 2.31 computes the chunk's chunk-skipping ranges only under
//! this removal, since the others delete the rows before the ranges are read.
//! Its truncate needs an `AccessExclusiveLock` at the end, and an `UPDATE` or
//! `DELETE` of history waiting on the chunk, wherever its rows lie, since
//! `TimescaleDB` locks every chunk of the hypertable for one, deadlocks it.
//! The conversion is then rolled back to the savepoint and run again at once
//! with `'truncate_or_delete'`, which truncates only when the lock is free and
//! deletes the rows otherwise, so it never deadlocks. A chunk converted that
//! way records no range and is never excluded by chunk skipping, which costs a
//! read nothing it would not cost without chunk skipping, and steady history
//! changes never keep a chunk from converting. The rowstore indexes a deletion
//! leaves at full size are reclaimed once vacuum has emptied the heap
//! ([`crate::rowstore_reclaim`]).
//!
//! A conversion that meets a deadlock or waits out its lock timeout is
//! deferred to the next sweep rather than failing it.

use crate::{
    bounded_lock_wait::lock_timeout_statement,
    columnstore_capability::ColumnstoreSupport,
    error::TemporalStoreError,
    hypertable_policy::HypertableName,
    repo::TemporalRepository,
    rowstore_history_chunks::{SegmentCensus, keep_statement, kept_chunks_statement, prune_statement},
};
use andromeda_postgresql_pool::{schema_change_lock::is_lock_not_available, transaction_rollback::is_transaction_rolled_back};
use andromeda_postgresql_seaquery::{
    advisory_lock::try_advisory_xact_lock,
    columnstore_chunk::{RowCount, SegmentCount, segment_census_statement},
    columnstore_conversion::{ChunkReference, ConvertToColumnstore, RowstoreRemoval},
    command::Command,
    setting::lift_statement_timeout,
    statement::Statement,
    timescale::{ChunkIsCompressed, ChunkNameColumn, ChunkRangeEnd, ChunkRangeStart, ChunkRelationColumn, ChunkSchemaColumn, chunk_states_statement},
};
use chrono::{DateTime, TimeDelta, Utc};
use derive_more::Display;
use sea_query::{Expr, Iden, Query, SelectStatement, error::Error as SeaQueryError};
use sqlx::{Connection, PgConnection, Row};
use std::num::NonZeroU32;
use thiserror::Error;

/// The hypertable the sweep converts.
const CONVERTED_HYPERTABLE: HypertableName = HypertableName::AttributeHistory;

/// One history chunk, as the catalogue names it.
#[derive(Clone, Debug, Display, Eq, PartialEq)]
#[display("{relation}")]
pub struct HistoryChunk {
    /// The relation as `format('%I.%I', chunk_schema, chunk_name)` renders it.
    relation: String,
    /// The schema the relation lives in.
    schema: String,
    /// The relation's name.
    name: String,
    /// The chunk's start on the time dimension, which names a kept chunk.
    range_start: DateTime<Utc>,
}

/// Where one chunk's rows are stored.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChunkStorage {
    /// Uncompressed rows, as written.
    Rowstore,
    /// Converted to the columnstore.
    Columnstore,
}

/// One chunk as the catalogue reports it.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ChunkState {
    /// The chunk.
    chunk: HistoryChunk,
    /// The first instant past the chunk on the time dimension.
    range_end: DateTime<Utc>,
    /// Where its rows are stored.
    storage: ChunkStorage,
}

/// The age past which a history chunk is converted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ColumnstoreHorizon(NonZeroU32);

impl ColumnstoreHorizon {
    /// A horizon of `days` whole days.
    #[must_use]
    pub const fn of_days(days: NonZeroU32) -> ColumnstoreHorizon {
        ColumnstoreHorizon(days)
    }

    /// The instant a chunk must end at or before to be converted.
    ///
    /// # Errors
    ///
    /// Returns [`ColumnstoreHorizonOutOfRange`] when the horizon reaches past
    /// the representable range from `now`.
    pub fn cutoff(self, now: DateTime<Utc>) -> Result<DateTime<Utc>, ColumnstoreHorizonOutOfRange> {
        TimeDelta::try_days(i64::from(self.0.get()))
            .and_then(|span| now.checked_sub_signed(span))
            .ok_or(ColumnstoreHorizonOutOfRange)
    }
}

/// A conversion horizon reaching past the representable range of instants.
#[derive(Clone, Copy, Debug, Eq, Error, Hash, PartialEq)]
#[error("temporal.columnstore_after reaches past the representable range of instants")]
pub struct ColumnstoreHorizonOutOfRange;

/// The chunks one sweep converts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversionCandidates {
    /// The edition holds no columnstore, so nothing is ever converted.
    Unsupported,
    /// The rowstore chunks ending at or before `cutoff` and not kept in the
    /// rowstore, oldest first.
    Due {
        /// The instant the chunks end at or before.
        cutoff: DateTime<Utc>,
        /// The chunks, oldest first.
        chunks: Vec<HistoryChunk>,
    },
}

/// What one sweep did with one chunk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChunkConversion {
    /// The chunk was converted.
    Converted,
    /// The chunk holds too few rows per segment for the columnstore to pay,
    /// and is kept in the rowstore for good.
    KeptInRowstore,
    /// Another replica holds the chunk's lock, or converted or kept it already.
    PassedOver,
    /// The conversion met a deadlock or waited out its lock timeout and was
    /// rolled back; the next sweep tries again.
    Deferred,
}

impl TemporalRepository {
    /// The history chunks a sweep at `now` converts under `horizon`.
    ///
    /// Records of kept chunks retention has since dropped are pruned on the
    /// way.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the chunk catalogue cannot
    /// be read or the kept chunks cannot be read or pruned, and
    /// [`TemporalStoreError::ColumnstoreHorizon`] when the horizon names no
    /// instant.
    pub async fn columnstore_conversion_candidates(&self, horizon: ColumnstoreHorizon, now: DateTime<Utc>) -> Result<ConversionCandidates, TemporalStoreError> {
        match self.columnstore {
            ColumnstoreSupport::Absent => Ok(ConversionCandidates::Unsupported),
            ColumnstoreSupport::Present => {
                let cutoff = horizon.cutoff(now)?;
                let mut connection = self.pool.acquire().await?;
                let states = read_chunk_states(&mut connection).await?;
                let starts: Vec<DateTime<Utc>> = states.iter().map(|state| state.chunk.range_start).collect();
                prune_statement(&starts).to_query().execute(&mut *connection).await?;
                let kept = read_kept_chunks(&mut connection).await?;
                Ok(ConversionCandidates::Due {
                    cutoff,
                    chunks: past_horizon(states, &kept, cutoff),
                })
            }
        }
    }

    /// Convert one chunk [`columnstore_conversion_candidates`] selected, or
    /// keep it in the rowstore when converting it would not pay.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when a statement fails for a
    /// reason other than a deadlock or a lock timeout.
    ///
    /// [`columnstore_conversion_candidates`]: TemporalRepository::columnstore_conversion_candidates
    pub async fn convert_chunk_to_columnstore(&self, chunk: &HistoryChunk) -> Result<ChunkConversion, TemporalStoreError> {
        let mut transaction = self.pool.begin().await?;
        match convert_under_lock(&mut transaction, chunk).await {
            Ok(outcome @ (ChunkConversion::Converted | ChunkConversion::KeptInRowstore)) => {
                transaction.commit().await?;
                Ok(outcome)
            }
            Ok(outcome @ (ChunkConversion::PassedOver | ChunkConversion::Deferred)) => {
                transaction.rollback().await?;
                Ok(outcome)
            }
            Err(ChunkConversionFailure::Database(error)) if defers_conversion(&error) => {
                transaction.rollback().await?;
                Ok(ChunkConversion::Deferred)
            }
            Err(ChunkConversionFailure::Database(error)) => Err(error.into()),
            Err(ChunkConversionFailure::StatementBuild(error)) => Err(error.into()),
        }
    }
}

/// Why handling one chunk failed.
#[derive(Debug, Error)]
enum ChunkConversionFailure {
    /// A statement failed.
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    /// A statement could not be built.
    #[error(transparent)]
    StatementBuild(#[from] SeaQueryError),
}

/// Take the chunk's lock, confirm it is still a rowstore chunk nobody kept,
/// count it, and keep or convert it.
async fn convert_under_lock(connection: &mut PgConnection, chunk: &HistoryChunk) -> Result<ChunkConversion, ChunkConversionFailure> {
    let locked = chunk_lock_statement(chunk).to_query_scalar::<bool>().fetch_one(&mut *connection).await?;
    if !locked {
        return Ok(ChunkConversion::PassedOver);
    }
    let still_rowstore = read_chunk_states(connection)
        .await?
        .iter()
        .any(|state| state.chunk == *chunk && state.storage == ChunkStorage::Rowstore);
    if !still_rowstore || read_kept_chunks(connection).await?.contains(&chunk.range_start) {
        return Ok(ChunkConversion::PassedOver);
    }
    for statement in conversion_settings() {
        statement.to_query().execute(&mut *connection).await?;
    }
    let census = count_segments(connection, chunk).await?;
    if !CONVERTED_HYPERTABLE.segment_break_even().pays(census.rows, census.segments) {
        keep_statement(chunk.range_start, census)?.to_query().execute(&mut *connection).await?;
        return Ok(ChunkConversion::KeptInRowstore);
    }
    match convert_within_savepoint(connection, chunk, RowstoreRemoval::Truncate).await {
        Ok(()) => Ok(ChunkConversion::Converted),
        Err(error) if defers_conversion(&error) => {
            convert_within_savepoint(connection, chunk, RowstoreRemoval::TruncateWhenFree).await?;
            Ok(ChunkConversion::Converted)
        }
        Err(error) => Err(error.into()),
    }
}

/// Convert `chunk` removing its rowstore rows by `removal`, under a savepoint
/// that a failed conversion is rolled back to, which releases every lock the
/// conversion took and leaves the enclosing transaction usable.
async fn convert_within_savepoint(connection: &mut PgConnection, chunk: &HistoryChunk, removal: RowstoreRemoval) -> Result<(), sqlx::Error> {
    let mut savepoint = connection.begin().await?;
    let converted = ConvertToColumnstore::new(
        ChunkReference::Name(chunk.relation.clone()), // clone: the command owns the name it quotes
        removal,
    )
    .to_query()
    .execute(&mut *savepoint)
    .await;
    match converted {
        Ok(_) => savepoint.commit().await,
        Err(error) => {
            savepoint.rollback().await?;
            Err(error)
        }
    }
}

/// Count the rows of `chunk` and the segments the layout would split them into.
async fn count_segments(connection: &mut PgConnection, chunk: &HistoryChunk) -> Result<SegmentCensus, sqlx::Error> {
    let segment_by = CONVERTED_HYPERTABLE.columnstore_layout().segment_by;
    let row = segment_census_statement(&chunk.schema, &chunk.name, segment_by)
        .to_query()
        .fetch_one(connection)
        .await?;
    Ok(SegmentCensus {
        rows: count(row.try_get(RowCount.unquoted())?),
        segments: count(row.try_get(SegmentCount.unquoted())?),
    })
}

/// A `count(*)`, which is never negative.
fn count(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

/// Read every chunk of the converted hypertable.
async fn read_chunk_states(connection: &mut PgConnection) -> Result<Vec<ChunkState>, sqlx::Error> {
    let rows = chunk_states_statement(CONVERTED_HYPERTABLE.into()).to_query().fetch_all(connection).await?;
    rows.iter()
        .map(|row| {
            let compressed: bool = row.try_get(ChunkIsCompressed.unquoted())?;
            Ok(ChunkState {
                chunk: HistoryChunk {
                    relation: row.try_get(ChunkRelationColumn.unquoted())?,
                    schema: row.try_get(ChunkSchemaColumn.unquoted())?,
                    name: row.try_get(ChunkNameColumn.unquoted())?,
                    range_start: row.try_get(ChunkRangeStart.unquoted())?,
                },
                range_end: row.try_get(ChunkRangeEnd.unquoted())?,
                storage: if compressed { ChunkStorage::Columnstore } else { ChunkStorage::Rowstore },
            })
        })
        .collect()
}

/// Read the starts of the chunks kept in the rowstore.
async fn read_kept_chunks(connection: &mut PgConnection) -> Result<Vec<DateTime<Utc>>, sqlx::Error> {
    kept_chunks_statement().to_query_scalar().fetch_all(connection).await
}

/// The rowstore chunks whose whole range ends at or before `cutoff` and that
/// `kept` does not list, in the order the catalogue listed them.
fn past_horizon(states: Vec<ChunkState>, kept: &[DateTime<Utc>], cutoff: DateTime<Utc>) -> Vec<HistoryChunk> {
    states
        .into_iter()
        .filter(|state| state.storage == ChunkStorage::Rowstore && state.range_end <= cutoff && !kept.contains(&state.chunk.range_start))
        .map(|state| state.chunk)
        .collect()
}

/// `SELECT pg_try_advisory_xact_lock(hashtext('andromeda:columnstore-conversion:<chunk>'))`.
fn chunk_lock_statement(chunk: &HistoryChunk) -> SelectStatement {
    Query::select()
        .expr(try_advisory_xact_lock(Expr::val(format!("andromeda:columnstore-conversion:{chunk}"))))
        .to_owned()
}

/// The transaction-scoped settings one conversion runs under, in order. The
/// conversion command sets the truncate behaviour itself.
fn conversion_settings() -> [SelectStatement; 2] {
    [Query::select().expr(lift_statement_timeout()).to_owned(), lock_timeout_statement()]
}

/// Whether a conversion failed in a way the next sweep can overcome: a
/// transaction rolled back as a deadlock or serialization victim, or a lock
/// wait cut short.
fn defers_conversion(error: &sqlx::Error) -> bool {
    is_transaction_rolled_back(error) || is_lock_not_available(error)
}

#[cfg(test)]
mod tests {
    use crate::columnstore_conversion::{
        ChunkState,
        ChunkStorage,
        ColumnstoreHorizon,
        ColumnstoreHorizonOutOfRange,
        HistoryChunk,
        chunk_lock_statement,
        conversion_settings,
        defers_conversion,
        past_horizon,
    };
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use andromeda_postgresql_pool::fake_database_error::FakeDatabaseError;
    use chrono::{DateTime, TimeDelta, Utc};
    use pretty_assertions::{assert_eq, assert_ne};
    use sea_query::PostgresQueryBuilder;
    use std::num::NonZeroU32;

    /// An instant fixed for every case.
    fn instant(text: &str) -> DateTime<Utc> {
        text.parse::<NgsiLdDateTime>().expect("an RFC 3339 instant").get()
    }

    /// The chunk named `name`, starting at `range_start`.
    fn history_chunk(name: &str, range_start: DateTime<Utc>) -> HistoryChunk {
        HistoryChunk {
            relation: format!("_timescaledb_internal.{name}"),
            schema: "_timescaledb_internal".to_owned(),
            name: name.to_owned(),
            range_start,
        }
    }

    /// A chunk named `name` spanning the seven days before `range_end`.
    fn chunk(name: &str, range_end: &str, storage: ChunkStorage) -> ChunkState {
        let end = instant(range_end);
        ChunkState {
            chunk: history_chunk(name, end - TimeDelta::days(7)),
            range_end: end,
            storage,
        }
    }

    /// The relations of `chunks`, as the sweep names them.
    fn relations(chunks: &[HistoryChunk]) -> Vec<String> {
        chunks.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_deadlock_serialization_failure_or_refused_lock_defers_the_conversion() {
        for code in ["40P01", "40001", "55P03"] {
            assert!(defers_conversion(&FakeDatabaseError::new("contended", code).into_sqlx()), "{code}");
        }
        for code in ["23505", "57014"] {
            assert!(!defers_conversion(&FakeDatabaseError::new("refused", code).into_sqlx()), "{code}");
        }
        assert!(!defers_conversion(&sqlx::Error::PoolTimedOut));
    }

    #[test]
    fn the_cutoff_lies_the_horizon_before_now() {
        let horizon = ColumnstoreHorizon::of_days(NonZeroU32::new(30).expect("thirty"));
        let now = instant("2026-10-17T12:00:00Z");
        assert_eq!(horizon.cutoff(now), Ok(now - TimeDelta::days(30)));
        assert_eq!(
            ColumnstoreHorizon::of_days(NonZeroU32::MAX).cutoff(DateTime::<Utc>::MIN_UTC),
            Err(ColumnstoreHorizonOutOfRange)
        );
    }

    #[test]
    fn only_rowstore_chunks_ending_at_or_before_the_cutoff_are_selected_oldest_first() {
        let cutoff = instant("2026-09-17T12:00:00Z");
        let states = vec![
            chunk("_hyper_1_1_chunk", "2026-08-20T00:00:00Z", ChunkStorage::Columnstore),
            chunk("_hyper_1_2_chunk", "2026-08-27T00:00:00Z", ChunkStorage::Rowstore),
            chunk("_hyper_1_3_chunk", "2026-09-17T12:00:00Z", ChunkStorage::Rowstore),
            chunk("_hyper_1_4_chunk", "2026-09-17T12:00:01Z", ChunkStorage::Rowstore),
            chunk("_hyper_1_5_chunk", "2026-10-22T00:00:00Z", ChunkStorage::Rowstore),
        ];
        assert_eq!(
            relations(&past_horizon(states, &[], cutoff)),
            vec!["_timescaledb_internal._hyper_1_2_chunk", "_timescaledb_internal._hyper_1_3_chunk"]
        );
    }

    #[test]
    fn a_converted_chunk_is_never_selected_however_old() {
        let states = vec![
            chunk("_hyper_1_1_chunk", "2020-01-01T00:00:00Z", ChunkStorage::Columnstore),
            chunk("_hyper_1_6_chunk", "2021-01-01T00:00:00Z", ChunkStorage::Columnstore),
        ];
        assert_eq!(past_horizon(states, &[], instant("2026-10-17T00:00:00Z")), Vec::<HistoryChunk>::new());
    }

    #[test]
    fn a_chunk_kept_in_the_rowstore_is_never_selected_again() {
        let states = vec![
            chunk("_hyper_1_2_chunk", "2026-08-27T00:00:00Z", ChunkStorage::Rowstore),
            chunk("_hyper_1_3_chunk", "2026-09-03T00:00:00Z", ChunkStorage::Rowstore),
        ];
        let kept = [instant("2026-08-20T00:00:00Z")];
        assert_eq!(
            relations(&past_horizon(states, &kept, instant("2026-10-17T00:00:00Z"))),
            vec!["_timescaledb_internal._hyper_1_3_chunk"]
        );
    }

    #[test]
    fn replicas_contend_for_a_chunk_without_waiting_and_only_for_the_same_chunk() {
        let one = history_chunk("_hyper_1_2_chunk", instant("2026-08-20T00:00:00Z"));
        let other = history_chunk("_hyper_1_3_chunk", instant("2026-08-27T00:00:00Z"));
        let (sql, values) = chunk_lock_statement(&one).build(PostgresQueryBuilder);
        assert!(sql.contains("pg_try_advisory_xact_lock(hashtext("), "{sql}");
        assert_eq!(values, chunk_lock_statement(&one).build(PostgresQueryBuilder).1);
        assert_ne!(values, chunk_lock_statement(&other).build(PostgresQueryBuilder).1);
    }

    #[test]
    fn a_conversion_runs_unbounded_with_a_bounded_lock_wait() {
        let rendered: Vec<String> = conversion_settings()
            .iter()
            .map(|statement| statement.to_string(PostgresQueryBuilder))
            .collect();
        assert_eq!(
            rendered,
            vec![
                "SELECT set_config('statement_timeout', '0', TRUE)",
                "SELECT set_config('lock_timeout', '2000ms', TRUE)",
            ]
        );
    }
}
