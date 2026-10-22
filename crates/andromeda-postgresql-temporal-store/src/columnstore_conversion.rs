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
//! (`temporal.columnstore_after` before now) and it is still in the rowstore; a
//! converted chunk is never selected again. Each chunk is converted in a
//! transaction of its own, so a lock is held only for one chunk, under these
//! transaction-scoped settings:
//!
//! - an advisory lock keyed on the chunk, taken without waiting: replicas of
//!   one broker sweep the same tenant, and the one that holds the lock converts
//!   the chunk while the others pass over it; the chunk's state is read again
//!   under the lock, since another replica may have converted it in between;
//! - no `statement_timeout`: a conversion takes about 20 to 25 s per GB the
//!   chunk holds, and no request waits on it;
//! - a bounded `lock_timeout`: a conversion queued behind a long transaction
//!   would hold up every write that reaches the chunk after it;
//! - `timescaledb.compress_truncate_behaviour = 'truncate_disabled'`: the
//!   rowstore rows are deleted rather than the relation truncated, so the
//!   conversion never takes the `AccessExclusiveLock` a truncate needs. With
//!   truncation, an `UPDATE` or `DELETE` waiting on the chunk deadlocks against
//!   that last lock and PostgreSQL aborts the conversion, losing its work; the
//!   deleted rows are left for autovacuum instead.
//!
//! A conversion that still meets a deadlock or waits out its lock timeout is
//! deferred to the next sweep rather than failing it.

use crate::{
    bounded_lock_wait::lock_timeout_statement,
    columnstore_capability::ColumnstoreSupport,
    error::TemporalStoreError,
    hypertable_policy::HypertableName,
    repo::TemporalRepository,
};
use andromeda_postgresql_pool::{schema_change_lock::is_lock_not_available, transaction_rollback::is_transaction_rolled_back};
use andromeda_postgresql_seaquery::{
    advisory_lock::try_advisory_xact_lock,
    columnstore_conversion::{ChunkReference, ConvertToColumnstore},
    command::Command,
    setting::lift_statement_timeout,
    statement::Statement,
    timescale::chunk_states_statement,
};
use chrono::{DateTime, TimeDelta, Utc};
use derive_more::Display;
use sea_query::{Expr, Query, SelectStatement};
use sqlx::{PgConnection, Row};
use std::num::NonZeroU32;
use thiserror::Error;

/// The hypertable the sweep converts.
const CONVERTED_HYPERTABLE: HypertableName = HypertableName::AttributeHistory;

/// One chunk relation, as the schema-qualified, quoted name `TimescaleDB`'s
/// chunk catalogue reports.
#[derive(Clone, Debug, Display, Eq, PartialEq)]
pub struct ChunkRelation(String);

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
    /// The chunk relation.
    relation: ChunkRelation,
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
    /// The rowstore chunks ending at or before `cutoff`, oldest first.
    Due {
        /// The instant the chunks end at or before.
        cutoff: DateTime<Utc>,
        /// The chunks, oldest first.
        chunks: Vec<ChunkRelation>,
    },
}

/// What converting one chunk did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChunkConversion {
    /// The chunk was converted.
    Converted,
    /// Another replica holds the chunk's lock, or converted it already.
    PassedOver,
    /// The conversion met a deadlock or waited out its lock timeout and was
    /// rolled back; the next sweep tries again.
    Deferred,
}

impl TemporalRepository {
    /// The history chunks a sweep at `now` converts under `horizon`.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the chunk catalogue cannot
    /// be read and [`TemporalStoreError::ColumnstoreHorizon`] when the horizon
    /// names no instant.
    pub async fn columnstore_conversion_candidates(&self, horizon: ColumnstoreHorizon, now: DateTime<Utc>) -> Result<ConversionCandidates, TemporalStoreError> {
        match self.columnstore {
            ColumnstoreSupport::Absent => Ok(ConversionCandidates::Unsupported),
            ColumnstoreSupport::Present => {
                let cutoff = horizon.cutoff(now)?;
                let mut connection = self.pool.acquire().await?;
                let states = read_chunk_states(&mut connection).await?;
                Ok(ConversionCandidates::Due {
                    cutoff,
                    chunks: past_horizon(states, cutoff),
                })
            }
        }
    }

    /// Convert one chunk [`columnstore_conversion_candidates`] selected.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when a statement fails for a
    /// reason other than a deadlock or a lock timeout.
    ///
    /// [`columnstore_conversion_candidates`]: TemporalRepository::columnstore_conversion_candidates
    pub async fn convert_chunk_to_columnstore(&self, chunk: &ChunkRelation) -> Result<ChunkConversion, TemporalStoreError> {
        let mut transaction = self.pool.begin().await?;
        match convert_under_lock(&mut transaction, chunk).await {
            Ok(ChunkConversion::Converted) => {
                transaction.commit().await?;
                Ok(ChunkConversion::Converted)
            }
            Ok(outcome @ (ChunkConversion::PassedOver | ChunkConversion::Deferred)) => {
                transaction.rollback().await?;
                Ok(outcome)
            }
            Err(error) if defers_conversion(&error) => {
                transaction.rollback().await?;
                Ok(ChunkConversion::Deferred)
            }
            Err(error) => Err(error.into()),
        }
    }
}

/// Take the chunk's lock, confirm it is still in the rowstore, and convert it.
async fn convert_under_lock(connection: &mut PgConnection, chunk: &ChunkRelation) -> Result<ChunkConversion, sqlx::Error> {
    let locked = chunk_lock_statement(chunk).to_query_scalar::<bool>().fetch_one(&mut *connection).await?;
    if !locked {
        return Ok(ChunkConversion::PassedOver);
    }
    let still_rowstore = read_chunk_states(connection)
        .await?
        .iter()
        .any(|state| state.relation == *chunk && state.storage == ChunkStorage::Rowstore);
    if !still_rowstore {
        return Ok(ChunkConversion::PassedOver);
    }
    for statement in conversion_settings() {
        statement.to_query().execute(&mut *connection).await?;
    }
    ConvertToColumnstore::new(ChunkReference::Name(chunk.0.clone())) // clone: the command owns the name it quotes
        .to_query()
        .execute(&mut *connection)
        .await?;
    Ok(ChunkConversion::Converted)
}

/// Read every chunk of the converted hypertable.
async fn read_chunk_states(connection: &mut PgConnection) -> Result<Vec<ChunkState>, sqlx::Error> {
    let rows = chunk_states_statement(CONVERTED_HYPERTABLE.into()).to_query().fetch_all(connection).await?;
    rows.iter()
        .map(|row| {
            let compressed: bool = row.try_get("is_compressed")?;
            Ok(ChunkState {
                relation: ChunkRelation(row.try_get("chunk")?),
                range_end: row.try_get("range_end")?,
                storage: if compressed { ChunkStorage::Columnstore } else { ChunkStorage::Rowstore },
            })
        })
        .collect()
}

/// The rowstore chunks whose whole range ends at or before `cutoff`, in the
/// order the catalogue listed them.
fn past_horizon(states: Vec<ChunkState>, cutoff: DateTime<Utc>) -> Vec<ChunkRelation> {
    states
        .into_iter()
        .filter(|state| state.storage == ChunkStorage::Rowstore && state.range_end <= cutoff)
        .map(|state| state.relation)
        .collect()
}

/// `SELECT pg_try_advisory_xact_lock(hashtext('andromeda:columnstore-conversion:<chunk>'))`.
fn chunk_lock_statement(chunk: &ChunkRelation) -> SelectStatement {
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
        ChunkRelation,
        ChunkState,
        ChunkStorage,
        ColumnstoreHorizon,
        ColumnstoreHorizonOutOfRange,
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

    /// A chunk named `name` ending at `range_end`.
    fn chunk(name: &str, range_end: &str, storage: ChunkStorage) -> ChunkState {
        ChunkState {
            relation: ChunkRelation(format!("_timescaledb_internal.{name}")),
            range_end: instant(range_end),
            storage,
        }
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
            past_horizon(states, cutoff),
            vec![
                ChunkRelation("_timescaledb_internal._hyper_1_2_chunk".to_owned()),
                ChunkRelation("_timescaledb_internal._hyper_1_3_chunk".to_owned()),
            ]
        );
    }

    #[test]
    fn a_converted_chunk_is_never_selected_however_old() {
        let states = vec![
            chunk("_hyper_1_1_chunk", "2020-01-01T00:00:00Z", ChunkStorage::Columnstore),
            chunk("_hyper_1_6_chunk", "2021-01-01T00:00:00Z", ChunkStorage::Columnstore),
        ];
        assert_eq!(past_horizon(states, instant("2026-10-17T00:00:00Z")), Vec::<ChunkRelation>::new());
    }

    #[test]
    fn replicas_contend_for_a_chunk_without_waiting_and_only_for_the_same_chunk() {
        let one = ChunkRelation("_timescaledb_internal._hyper_1_2_chunk".to_owned());
        let other = ChunkRelation("_timescaledb_internal._hyper_1_3_chunk".to_owned());
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
